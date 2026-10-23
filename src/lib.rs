#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)
)]

use serde::Serialize;
use tree_sitter::{Node, Parser};

// strictrs: capability
pub mod capability;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Report {
    pub ok: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: String,
    pub source: String,
    pub code: String,
    pub message: String,
    pub at: Location,
    pub fixes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub snippet: String,
}

pub fn report(mut diagnostics: Vec<Diagnostic>) -> Report {
    diagnostics.sort_by(|left, right| {
        (
            &left.at.file,
            left.at.line,
            left.at.col,
            &left.code,
            &left.message,
        )
            .cmp(&(
                &right.at.file,
                right.at.line,
                right.at.col,
                &right.code,
                &right.message,
            ))
    });
    diagnostics.dedup();
    Report {
        ok: diagnostics.is_empty(),
        error_count: diagnostics.len(),
        warning_count: 0,
        diagnostics,
    }
}

pub fn diagnostic(
    code: &str,
    message: &str,
    file: &str,
    source: &str,
    node: Node<'_>,
) -> Diagnostic {
    let start = node.start_position();
    let end = node.end_position();
    Diagnostic {
        level: "error".to_owned(),
        source: "strictsh".to_owned(),
        code: format!("strictsh::{code}"),
        message: message.to_owned(),
        at: Location {
            file: file.to_owned(),
            line: start.row + 1,
            col: source
                .get(node.start_byte().saturating_sub(start.column)..node.start_byte())
                .map_or(1, |prefix| prefix.chars().count() + 1),
            end_line: end.row + 1,
            end_col: source
                .get(node.end_byte().saturating_sub(end.column)..node.end_byte())
                .map_or(1, |prefix| prefix.chars().count() + 1),
            snippet: source.lines().nth(start.row).unwrap_or_default().to_owned(),
        },
        fixes: Vec::new(),
    }
}

fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}

pub fn lint_source(file: &str, source: &str) -> Result<Vec<Diagnostic>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .map_err(|error| format!("cannot load Bash grammar: {error}"))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "Bash parser cancelled".to_owned())?;
    let root = tree.root_node();
    let mut diagnostics = Vec::new();
    let mut add = |code: &str, message: &str, node: Node<'_>| {
        diagnostics.push(diagnostic(code, message, file, source, node));
    };
    if !matches!(
        source.lines().next(),
        Some("#!/bin/sh" | "#!/usr/bin/sh" | "#!/usr/bin/env sh")
    ) {
        add(
            "sh_shebang",
            "start with #!/bin/sh (or an absolute sh shebang)",
            root,
        );
    }
    let mut cursor = root.walk();
    let first = root
        .named_children(&mut cursor)
        .find(|node| node.kind() != "comment");
    if first.map(|node| text(node, source)) != Some("set -u") {
        add(
            "required_options",
            "the first executable statement must be exactly: set -u",
            root,
        );
    }
    visit(root, source, first.map(|node| node.id()), &mut add);
    Ok(diagnostics)
}

fn visit<'tree>(
    node: Node<'tree>,
    source: &str,
    prologue: Option<usize>,
    add: &mut impl FnMut(&str, &str, Node<'tree>),
) -> () {
    if node.is_error() || node.is_missing() {
        add(
            "syntax_error",
            "invalid or unsupported Bash syntax; also inspect ShellCheck's parse diagnostics",
            node,
        );
    }
    match node.kind() {
        "pipeline" => {
            add("no_pipeline", "portable sh cannot expose producer failures; use checked commands and temporary files", node);
            if !guarded(node) {
                add("explicit_status", "consume this status in if/while/until, or with an explicit || handler", node);
            }
        }
        "array" | "subscript" | "c_style_for_statement" | "here_string_redirect" => {
            add("posix_syntax", "Bash-specific syntax is forbidden; use portable POSIX sh", node);
        }
        "test_command" if text(node, source).starts_with("[[") || text(node, source).starts_with("((") => {
            add("posix_syntax", "use POSIX [ ... ] tests or $((...)) arithmetic expansion", node);
        }
        "function_definition" if text(node, source).starts_with("function ") => {
            add("posix_syntax", "declare POSIX functions with name() { ...; }", node);
        }
        "comment" => {
            if text(node, source).trim_start_matches('#').trim_start().starts_with("shellcheck") {
                add("no_suppression", "ShellCheck directives are forbidden; repair diagnostics instead of suppressing them", node);
            }
        }
        "command_substitution" => {
            if text(node, source).starts_with('`') {
                add("no_backticks", "use $(...) instead of backtick command substitution", node);
            }
            if !status_assignment(node) {
                add("no_masked_status", "command substitution must be the sole substitution in a standalone checked assignment", node);
            }
        }
        "process_substitution" => add("no_process_substitution", "process substitution hides producer exit status; use a checked command and a temporary file", node),
        "expansion" => {
            if text(node, source).starts_with("${!") {
                add("no_indirection", "indirect variable expansion is forbidden; pass explicit values", node);
            }
            if text(node, source) == "${*}" {
                add("no_star_args", "preserve argument boundaries with \"${@}\"", node);
            }
        }
        "simple_expansion" => {
            if text(node, source) == "$*" {
                add("no_star_args", "preserve argument boundaries with \"${@}\"", node);
            }
        }
        "command" => check_command(node, source, prologue, add),
        "declaration_command" => {
            let declaration = text(node, source);
            if ["local", "declare", "typeset"].iter().any(|name| declaration.split_whitespace().next() == Some(name)) {
                add("posix_syntax", "local, declare and typeset are not POSIX; use explicit global variables or a subshell", node);
            }
            if contains(node, "command_substitution") {
                add("no_masked_status", "declare a variable separately, then assign command output in a checked assignment", node);
            }
            let mut cursor = node.walk();
            if node.named_children(&mut cursor).any(|child| {
                let argument = text(child, source);
                argument.starts_with('-') && !argument.starts_with("--") && argument.contains('n')
            }) {
                add("no_indirection", "nameref declarations are forbidden; pass explicit values", node);
            }
        }
        "variable_assignment" => {
            if contains(node, "command_substitution") && !has_statement_parent(node) && !guarded(node) {
                add("explicit_status", "check command-substitution assignment status with if or an explicit || handler", node);
            }
        }
        "test_command" => {
            if !guarded(node) {
                add("explicit_status", "consume this status in if/while/until, or with an explicit || handler", node);
            }
        }
        "redirected_statement" if !guarded(node) => {
            add("explicit_status", "check redirected statement status; opening a file can fail", node);
        }
        _ => {}
    }
    // '&' is syntax, never a search through quoted strings or heredoc payloads.
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "&" {
            add(
                "no_background",
                "background jobs hide failures; run synchronously or move orchestration into Rust",
                child,
            );
        }
        visit(child, source, prologue, add);
    }
}

fn contains(node: Node<'_>, kind: &str) -> bool {
    if node.kind() == kind {
        return true;
    }
    let mut cursor = node.walk();
    let result = node
        .named_children(&mut cursor)
        .any(|child| contains(child, kind));
    result
}

fn count_substitutions(node: Node<'_>) -> usize {
    if node.kind() == "command_substitution" {
        return 1;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .map(count_substitutions)
        .sum()
}

fn status_assignment(mut node: Node<'_>) -> bool {
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "string" | "concatenation" => node = parent,
            "variable_assignment" => {
                return !has_statement_parent(parent) && count_substitutions(parent) == 1
            }
            _ => return false,
        }
    }
    false
}

fn has_statement_parent(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| matches!(parent.kind(), "command" | "declaration_command"))
}

fn internal_handler(node: Node<'_>) -> bool {
    if node.kind() == "redirected_statement" {
        let mut cursor = node.walk();
        if node
            .children_by_field_name("redirect", &mut cursor)
            .any(|redirect| {
                if redirect.kind() != "heredoc_redirect" {
                    return false;
                }
                let mut cursor = redirect.walk();
                redirect.child_by_field_name("right").is_some()
                    || redirect
                        .named_children(&mut cursor)
                        .any(|child| internal_handler(child))
            })
        {
            return true;
        }
    }
    if node.kind() == "pipeline"
        && node
            .parent()
            .is_some_and(|parent| parent.kind() == "heredoc_redirect")
    {
        let mut cursor = node.walk();
        return node
            .named_children(&mut cursor)
            .next()
            .is_some_and(|child| child.kind() == "list");
    }
    false
}

fn guarded(node: Node<'_>) -> bool {
    guarded_from(node, true)
}

fn guarded_from(mut node: Node<'_>, consider_internal: bool) -> bool {
    if consider_internal && internal_handler(node) {
        return true;
    }
    while let Some(parent) = node.parent() {
        match parent.kind() {
            "list" => {
                let mut cursor = parent.walk();
                if parent
                    .named_children(&mut cursor)
                    .last()
                    .is_some_and(|last| last.id() != node.id())
                {
                    return true;
                }
                node = parent;
            }
            "elif_clause" => {
                // tree-sitter-bash does not label elif conditions with a field.
                // Only the last statement before the `then` token is tested.
                let mut cursor = parent.walk();
                return parent
                    .children(&mut cursor)
                    .take_while(|child| child.kind() != "then")
                    .filter(|child| child.is_named() && child.kind() != "comment")
                    .last()
                    .is_some_and(|last| last.id() == node.id());
            }
            "if_statement" | "while_statement" => {
                let mut cursor = parent.walk();
                return parent
                    .children_by_field_name("condition", &mut cursor)
                    .filter(|child| child.is_named())
                    .last()
                    .is_some_and(|last| last.id() == node.id());
            }
            "redirected_statement" => return guarded(parent),
            // The right side is the handler, not the left status it consumes.
            "heredoc_redirect" => {
                return parent
                    .parent()
                    .is_some_and(|wrapper| guarded_from(wrapper, false))
            }
            "pipeline" | "negated_command" => node = parent,
            "command_substitution" => {
                let mut cursor = parent.walk();
                if parent
                    .named_children(&mut cursor)
                    .last()
                    .is_none_or(|last| last.id() != node.id())
                {
                    return false;
                }
                node = parent;
            }
            "string" | "concatenation" | "variable_assignment" => node = parent,
            _ => return false,
        }
    }
    false
}

fn check_command<'tree>(
    node: Node<'tree>,
    source: &str,
    prologue: Option<usize>,
    add: &mut impl FnMut(&str, &str, Node<'tree>),
) -> () {
    let name = node
        .child_by_field_name("name")
        .map(|child| text(child, source))
        .unwrap_or_default();
    // Only literal command names are accepted. Escaped/quoted names must be made explicit.
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_-/.:".contains(ch))
    {
        add(
            "no_dynamic_command",
            "command names must be literal; use case to select a known command",
            node,
        );
    }
    let basename = name.rsplit('/').next().unwrap_or_default();
    match basename {
        "eval" | "source" | "." | "trap" => add("no_dynamic_code", "eval, source, dot sourcing and string traps are forbidden; keep scripts standalone and move complex logic to Rust", node),
        "echo" => add("prefer_printf", "use printf with a literal format instead of implementation-dependent echo", node),
        "set" | "shopt" => {
            if prologue != Some(node.id()) {
                add("fixed_options", "shell options are fixed by the prologue; do not enable errexit or change options later", node);
            }
        }
        "sh" => {
            let mut cursor = node.walk();
            let script = node.children_by_field_name("argument", &mut cursor).next();
            if !script.is_some_and(|arg| arg.kind() == "word" && text(arg, source).ends_with(".sh") && !text(arg, source).starts_with('-')) {
                add("no_dynamic_code", "sh may only execute a literal .sh path; no inline code, stdin scripts or dynamic script paths", node);
            }
        }
        "bash" | "zsh" | "ksh" | "dash" | "env" | "command" | "builtin" | "exec" => {
            add("no_dynamic_code", "other shell interpreters and command-dispatch wrappers are outside this profile; call a literal executable directly", node);
        }
        _ => {}
    }
    let permitted = matches!(
        name,
        "printf" | ":" | "true" | "false" | "return" | "exit" | "break" | "continue" | "set"
    );
    let in_pipeline = node
        .parent()
        .is_some_and(|parent| parent.kind() == "pipeline");
    if !permitted && !in_pipeline && !guarded(node) {
        add("explicit_status", "check this command with if/while/until, or an explicit || handler (e.g. command || exit 1)", node);
    }
    // Redirection can fail even on otherwise exempt output/control builtins.
    let mut cursor = node.walk();
    if permitted
        && node
            .children_by_field_name("redirect", &mut cursor)
            .next()
            .is_some()
        && !guarded(node)
    {
        add(
            "explicit_status",
            "check redirected command status; opening an output file can fail",
            node,
        );
    }
}
