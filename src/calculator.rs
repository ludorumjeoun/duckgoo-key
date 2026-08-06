use std::time::{Duration, Instant};

use crate::catalog::{CatalogItem, LaunchAction};

const PREFIX: char = '=';
const MAX_EXPRESSION_BYTES: usize = 512;
const MAX_RESULT_BYTES: usize = 8 * 1024;
const EVALUATION_BUDGET: Duration = Duration::from_millis(50);

struct EvaluationDeadline {
    started_at: Instant,
}

impl EvaluationDeadline {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }
}

impl fend_core::Interrupt for EvaluationDeadline {
    fn should_interrupt(&self) -> bool {
        self.started_at.elapsed() >= EVALUATION_BUDGET
    }
}

/// Builds an inline calculator result for an explicitly prefixed expression or
/// an unambiguous arithmetic expression.
///
/// `=` opts into the full calculator grammar. Without it, the expression must
/// begin with a number and contain an arithmetic operator or a unit conversion
/// so ordinary application and command searches stay free from false positives.
/// A fresh fend context also prevents one query from defining variables that
/// silently affect a later query.
pub fn calculator_item(query: &str) -> Option<CatalogItem> {
    let expression = expression_from_query(query)?;
    let result = evaluate(expression)?;

    Some(CatalogItem {
        id: format!("calculator:{expression}"),
        title: result.clone(),
        subtitle: Some(format!("Copy result · {expression}")),
        icon_path: None,
        keywords: vec!["calculator".to_owned(), "calculate".to_owned()],
        action: LaunchAction::CopyText { text: result },
        pinnable: false,
    })
}

fn expression_from_query(query: &str) -> Option<&str> {
    let query = query.trim();
    let expression = query
        .strip_prefix(PREFIX)
        .map(str::trim)
        .filter(|expression| !expression.is_empty())
        .or_else(|| looks_like_implicit_expression(query).then_some(query))?;

    (expression.len() <= MAX_EXPRESSION_BYTES).then_some(expression)
}

fn looks_like_implicit_expression(expression: &str) -> bool {
    let mut characters = expression.chars().peekable();
    let starts_with_number = matches!(characters.peek(), Some('0'..='9'))
        || matches!(characters.next(), Some('+') | Some('-'))
            && matches!(characters.next(), Some('0'..='9'));
    if !starts_with_number {
        return false;
    }

    expression
        .split_whitespace()
        .any(|word| word.eq_ignore_ascii_case("to"))
        || expression
            .chars()
            .any(|character| matches!(character, '+' | '-' | '*' | '/' | '%' | '^' | '×' | '÷'))
}

fn evaluate(expression: &str) -> Option<String> {
    let mut context = fend_core::Context::new();
    context.disable_rng();
    let deadline = EvaluationDeadline::new();
    let evaluated = fend_core::evaluate_with_interrupt(expression, &mut context, &deadline).ok()?;
    let result = evaluated.get_main_result().trim();

    (!result.is_empty() && result.len() <= MAX_RESULT_BYTES).then(|| result.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_an_explicit_arithmetic_expression() {
        let item = calculator_item("= 2 + 3 * 4").unwrap();

        assert_eq!(item.id, "calculator:2 + 3 * 4");
        assert_eq!(item.title, "14");
        assert_eq!(item.subtitle.as_deref(), Some("Copy result · 2 + 3 * 4"));
        assert_eq!(
            item.action,
            LaunchAction::CopyText {
                text: "14".to_owned()
            }
        );
        assert!(!item.pinnable);
    }

    #[test]
    fn supports_fend_units_without_persisting_context() {
        assert_eq!(
            calculator_item("= 1 hour to minutes").unwrap().title,
            "60 minutes"
        );
        assert!(calculator_item("= answer = 42").is_some());
        assert!(calculator_item("= answer").is_none());
    }

    #[test]
    fn evaluates_unambiguous_arithmetic_without_an_explicit_prefix() {
        assert_eq!(calculator_item("2 + 2").unwrap().title, "4");
        assert_eq!(
            calculator_item("1 hour to minutes").unwrap().title,
            "60 minutes"
        );
    }

    #[test]
    fn ignores_queries_that_are_not_unambiguous_calculations() {
        assert!(calculator_item("Calendar").is_none());
        assert!(calculator_item("2026 roadmap").is_none());
        assert!(calculator_item("2 notes").is_none());
        assert!(calculator_item("").is_none());
        assert!(calculator_item(" =   ").is_none());
        assert!(calculator_item("= definitely not valid (").is_none());
    }

    #[test]
    fn rejects_oversized_expressions_before_evaluation() {
        let query = format!("={}", "1".repeat(MAX_EXPRESSION_BYTES + 1));

        assert!(calculator_item(&query).is_none());
    }

    #[test]
    fn trims_the_trigger_and_expression_for_stable_identity() {
        let item = calculator_item("  =   6 / 3   ").unwrap();

        assert_eq!(item.id, "calculator:6 / 3");
        assert_eq!(item.title, "2");
    }
}
