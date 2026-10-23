use proptest::prelude::{prop_assert, prop_assert_eq, proptest};
use strictsh::{lint_source, report};

const HEADER: &str = "#!/bin/sh\nset -u\n";

fn codes(body: &str) -> Vec<String> {
    lint_source("test.sh", &format!("{HEADER}{body}\n"))
        .unwrap()
        .into_iter()
        .map(|item| item.code)
        .collect()
}

#[test]
fn every_rule_has_an_executable_example() {
    for (code, body) in [
        ("fixed_options", "set -e"),
        ("no_dynamic_code", "eval 'printf hi' || exit 1"),
        ("no_dynamic_code", "source ./lib.sh || exit 1"),
        ("no_dynamic_code", "trap 'printf bye' EXIT || exit 1"),
        ("no_dynamic_code", "bash -c 'printf hi' || exit 1"),
        ("no_dynamic_code", "/bin/bash -c 'printf hi' || exit 1"),
        ("no_dynamic_command", "\"${tool}\" || exit 1"),
        ("no_indirection", "printf '%s' \"${!name}\""),
        ("no_indirection", "declare -n ref=var"),
        ("no_indirection", "declare -gn ref=var"),
        ("no_backticks", "if out=`date`; then :; fi"),
        ("no_process_substitution", "cat <(date) || exit 1"),
        ("no_background", "date &"),
        ("no_masked_status", "local out=$(date)"),
        ("no_masked_status", "printf '%s' \"$(date)\""),
        ("no_masked_status", "out=\"$(date)$(date)\" || exit 1"),
        ("no_star_args", "printf '%s' \"$*\""),
        ("no_star_args", "printf '%s' \"${*}\""),
        ("prefer_printf", "echo hi || exit 1"),
        ("no_suppression", "# shellcheck disable=SC2086\nprintf hi"),
        ("syntax_error", "if then fi"),
        ("explicit_status", "curl https://example.com"),
    ] {
        assert!(
            codes(body).contains(&format!("strictsh::{code}")),
            "{code}: {body}"
        );
    }
    let bare = lint_source("bare.sh", "printf hi\n").unwrap();
    assert!(bare.iter().any(|item| item.code == "strictsh::sh_shebang"));
    assert!(bare
        .iter()
        .any(|item| item.code == "strictsh::required_options"));
}

#[test]
fn explicit_status_is_required_on_final_operands_and_in_function_bodies() {
    for body in [
        "date",
        "true && date",
        "false || date",
        "date | cat",
        "out=$(date)",
        "f() { date; }; f || exit 1",
        "if date; true; then :; fi",
        "{ date; } || exit 1",
        "if out=$(date; true); then :; fi",
        "printf hi >file",
        "printf hi >file\n",
    ] {
        assert!(
            codes(body)
                .iter()
                .any(|code| code == "strictsh::explicit_status"),
            "{body}"
        );
    }
}

#[test]
fn checked_conditions_and_assignments_are_accepted() {
    for body in [
        "date || exit 1",
        "if date; then :; else exit 1; fi",
        "while read -r line; do printf '%s' \"${line}\"; done",
        "until date; do break; done",
        "if out=$(date); then printf '%s' \"${out}\"; else exit 1; fi",
        "out=$(date) || exit 1",
        "printf hi >file || exit 1",
        "if [ -f file ]; then :; fi",
        "f() { date || return 1; }; f || exit 1",
    ] {
        assert!(codes(body).is_empty(), "{body}: {:?}", codes(body));
    }
}

#[test]
fn elif_conditions_are_handled_but_earlier_commands_and_bodies_are_not() {
    for body in [
        "if false; then :; elif date; then :; fi",
        "if false; then :; elif [ -f file ]; then :; fi",
        "if false; then :; elif out=$(date); then printf '%s' \"${out}\"; fi",
        "if false; then :; elif false; then :; elif date; then :; else :; fi",
        "if false; then :; elif date\n# comment before then\nthen :; fi",
    ] {
        assert!(codes(body).is_empty(), "{body}: {:?}", codes(body));
    }
    for (body, offending_command) in [
        ("if false; then :; elif date; true; then :; fi", "date"),
        ("if false; then :; elif true; then date; fi", "date"),
        ("if false; then :; elif true; then :; else date; fi", "date"),
    ] {
        let source = format!("{HEADER}{body}\n");
        let diagnostics = lint_source("test.sh", &source).unwrap();
        assert_eq!(diagnostics.len(), 1, "{body}: {diagnostics:?}");
        let item = diagnostics.first().unwrap();
        assert_eq!(item.code, "strictsh::explicit_status");
        assert_eq!(item.at.line, 3);
        assert_eq!(item.at.col, body.find(offending_command).unwrap() + 1);
        assert_eq!(item.at.end_col, item.at.col + offending_command.len());
    }
}

#[test]
fn inert_text_is_not_code_but_expanding_heredoc_substitutions_are() {
    assert!(codes("# eval date &\nprintf '%s' 'eval `date` <(date) & $*'\ncat <<'EOF' || exit 1\neval date &\nEOF").is_empty());
    assert!(codes("cat <<EOF || exit 1\n$(eval date)\nEOF")
        .contains(&"strictsh::no_dynamic_code".to_owned()));
}

#[test]
fn heredoc_handlers_and_pipelines_preserve_final_status_rules() {
    for body in [
        "cat <<EOF || exit 1\nx\nEOF",
        "if cat <<EOF\nx\nEOF\nthen :; fi",
    ] {
        assert!(codes(body).is_empty(), "{body}: {:?}", codes(body));
    }
    for body in [
        "cat <<EOF || date\nx\nEOF",
        "cat <<EOF | wc -l || date\nx\nEOF",
        "cat <<EOF | false\nx\nEOF",
    ] {
        assert!(
            codes(body).contains(&"strictsh::explicit_status".to_owned()),
            "{body}"
        );
    }
}

#[test]
fn spans_use_unicode_character_columns_and_sort_deterministically() {
    let source = format!("{HEADER}printf 'é'; echo hi || exit 1\n");
    let diagnostics = lint_source("z.sh", &source).unwrap();
    let echo = diagnostics
        .iter()
        .find(|item| item.code == "strictsh::prefer_printf")
        .unwrap();
    assert_eq!(echo.at.line, 3);
    assert_eq!(echo.at.col, 13);
    assert_eq!(echo.at.end_col, 20);
    let mut reversed = diagnostics.clone();
    reversed.reverse();
    assert_eq!(report(diagnostics), report(reversed));
}

proptest! {
    #[test]
    fn single_quoted_payload_is_inert(payload in "[a-zA-Z0-9 $`&;(){}!<>_-]{0,160}") {
        let source = format!("{HEADER}printf '%s' '{payload}'\n");
        let result = lint_source("property.sh", &source).unwrap();
        prop_assert!(result.is_empty(), "{:?}", result);
    }

    #[test]
    fn diagnostics_are_repeatable(body in "[a-zA-Z0-9 $`&;(){}!<>_'-]{0,160}") {
        let source = format!("{HEADER}{body}\n");
        prop_assert_eq!(lint_source("property.sh", &source), lint_source("property.sh", &source));
    }
}

#[test]
fn posix_profile_rejects_bashisms_and_even_guarded_pipelines() {
    for body in [
        "date | cat || exit 1",
        "if date | cat; then :; fi",
        "cat <<EOF | wc -l || exit 1\nx\nEOF",
    ] {
        assert!(
            codes(body).contains(&"strictsh::no_pipeline".to_owned()),
            "{body}"
        );
    }
    for body in [
        "if [[ -f file ]]; then :; fi",
        "items=(one two)",
        "local value=one",
        "function run { :; }",
    ] {
        assert!(
            codes(body).contains(&"strictsh::posix_syntax".to_owned()),
            "{body}"
        );
    }
    assert!(codes("printf '%s' '[[ array=(a b) | local ]]'").is_empty());
    assert!(codes("value=$((1 + 2))\nif [ \"${value}\" -eq 3 ]; then :; fi").is_empty());
}
