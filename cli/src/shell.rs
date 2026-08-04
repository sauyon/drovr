//! Quoting for the two kinds of shell command drovr produces: the ones it hands
//! to herdr's `pane run`, and the copy-pasteable remediation commands it PRINTS
//! for a human to run.
//!
//! Both are injection surfaces, and the second is the less obvious one: the
//! delivery mechanism is a person pasting drovr's own suggestion into their
//! shell. Run names, task names and pass tokens all reach drovr from argv or
//! from the review server's HTTP layer, and none of them is restricted to a
//! shell-safe alphabet (phase names now are — see `phase::require_phase_name` —
//! but that is a second, independent rule, not a reason to skip this one).
//!
//! Lives in its own module because three modules need it and a fourth copy of
//! `format!("'{}'", …)` is how one of them ends up subtly different.

/// POSIX single-quote `s` so it becomes exactly one literal shell word.
/// Neutralizes spaces and every metacharacter (`;`, `$()`, `&&`, backticks, …);
/// the enclosing quotes are stripped by the shell, so the value the command sees
/// is unchanged. An embedded single quote is escaped (`'\''`), not terminated.
pub(crate) fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_single_quote_neutralizes_metacharacters() {
        assert_eq!(shell_single_quote("a/b"), "'a/b'");
        assert_eq!(shell_single_quote("a; rm -rf ~"), "'a; rm -rf ~'");
        assert_eq!(shell_single_quote("$(id)"), "'$(id)'");
        assert_eq!(shell_single_quote("`id`"), "'`id`'");
        assert_eq!(shell_single_quote("a && b"), "'a && b'");
        assert_eq!(shell_single_quote(""), "''");
        // An embedded single quote is escaped, not terminated.
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        // …and the escape itself cannot be escaped out of: closing quote, a
        // backslash-quote, reopening quote — there is no way back to unquoted.
        assert_eq!(shell_single_quote("'; id; '"), "''\\''; id; '\\'''");
    }
}
