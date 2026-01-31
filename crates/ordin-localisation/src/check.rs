//! The one fixed check of person-layer text (Localisation.md section H, LOC-LF-2).
//!
//! `check_entry` reads only the compiled-in `en-GB` resource and ICU4X compiled data. It does no I/O.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Mutex, PoisonError};

use fluent_syntax::ast::{
    CallArguments, Entry, Expression, InlineExpression, Message, Pattern, PatternElement,
};
use fluent_syntax::parser;
use icu::locale::Locale;
use icu::plurals::{PluralCategory, PluralRules};

use crate::catalogue::english;
use crate::types::EntryProblem;

/// The longest entry, in characters (the `ftl` column check).
pub(crate) const MAX_ENTRY_CHARS: usize = 4_096;

/// What a message references, in document order where order matters.
#[derive(Debug, Default, Clone)]
pub(crate) struct Facts {
    pub(crate) variables: BTreeSet<String>,
    pub(crate) attributes: BTreeSet<String>,
    pub(crate) has_value: bool,
    pub(crate) references: Vec<Reference>,
    /// variable name → identifier keys used by a select on that variable.
    pub(crate) select_keys: BTreeMap<String, BTreeSet<String>>,
    /// (variable, key) in document order, for the plural check.
    pub(crate) selects: Vec<(String, Vec<String>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reference {
    Function(String),
    Message(String),
    Term(String),
}

/// Analyses one parsed message (value and attributes).
pub(crate) fn facts_of<S: AsRef<str> + Clone>(message: &Message<S>) -> Facts {
    let mut facts = Facts {
        has_value: message.value.is_some(),
        ..Facts::default()
    };
    if let Some(pattern) = &message.value {
        walk_pattern(pattern, &mut facts);
    }
    for attribute in &message.attributes {
        facts
            .attributes
            .insert(attribute.id.name.as_ref().to_string());
        walk_pattern(&attribute.value, &mut facts);
    }
    facts
}

fn walk_pattern<S: AsRef<str>>(pattern: &Pattern<S>, facts: &mut Facts) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            walk_expression(expression, facts);
        }
    }
}

fn walk_expression<S: AsRef<str>>(expression: &Expression<S>, facts: &mut Facts) {
    match expression {
        Expression::Inline(inline) => walk_inline(inline, facts),
        Expression::Select { selector, variants } => {
            walk_inline(selector, facts);
            if let Some(variable) = selector_variable(selector) {
                let keys: Vec<String> = variants
                    .iter()
                    .filter_map(|variant| match &variant.key {
                        fluent_syntax::ast::VariantKey::Identifier { name } => {
                            Some(name.as_ref().to_string())
                        }
                        fluent_syntax::ast::VariantKey::NumberLiteral { .. } => None,
                    })
                    .collect();
                facts
                    .select_keys
                    .entry(variable.clone())
                    .or_default()
                    .extend(keys.iter().cloned());
                facts.selects.push((variable, keys));
            }
            for variant in variants {
                walk_pattern(&variant.value, facts);
            }
        }
    }
}

/// The variable a select is on: `$name` or `NUMBER($name)`.
fn selector_variable<S: AsRef<str>>(selector: &InlineExpression<S>) -> Option<String> {
    match selector {
        InlineExpression::VariableReference { id } => Some(id.name.as_ref().to_string()),
        InlineExpression::FunctionReference { id, arguments } if id.name.as_ref() == "NUMBER" => {
            match arguments.positional.first() {
                Some(InlineExpression::VariableReference { id }) => {
                    Some(id.name.as_ref().to_string())
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn walk_arguments<S: AsRef<str>>(arguments: &CallArguments<S>, facts: &mut Facts) {
    for positional in &arguments.positional {
        walk_inline(positional, facts);
    }
    for named in &arguments.named {
        walk_inline(&named.value, facts);
    }
}

fn walk_inline<S: AsRef<str>>(inline: &InlineExpression<S>, facts: &mut Facts) {
    match inline {
        InlineExpression::StringLiteral { .. } | InlineExpression::NumberLiteral { .. } => {}
        InlineExpression::FunctionReference { id, arguments } => {
            facts
                .references
                .push(Reference::Function(id.name.as_ref().to_string()));
            walk_arguments(arguments, facts);
        }
        InlineExpression::MessageReference { id, .. } => {
            facts
                .references
                .push(Reference::Message(id.name.as_ref().to_string()));
        }
        InlineExpression::TermReference { id, arguments, .. } => {
            facts
                .references
                .push(Reference::Term(id.name.as_ref().to_string()));
            if let Some(arguments) = arguments {
                walk_arguments(arguments, facts);
            }
        }
        InlineExpression::VariableReference { id } => {
            facts.variables.insert(id.name.as_ref().to_string());
        }
        InlineExpression::Placeable { expression } => walk_expression(expression, facts),
    }
}

/// True for a C0 control other than TAB and LF, or a bidirectional override or isolate character.
fn is_forbidden(c: char) -> bool {
    (c.is_ascii_control() && c != '\t' && c != '\n')
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// The CLDR cardinal plural categories of `locale`; the root's `other` for a tag ICU4X does not know.
fn plural_categories(locale: &str) -> BTreeSet<&'static str> {
    static CACHE: Mutex<Option<HashMap<String, BTreeSet<&'static str>>>> = Mutex::new(None);
    {
        let guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = guard.as_ref().and_then(|map| map.get(locale)) {
            return found.clone();
        }
    }
    let mut categories: BTreeSet<&'static str> = BTreeSet::new();
    let rules = Locale::try_from_str(locale)
        .ok()
        .and_then(|parsed| PluralRules::try_new_cardinal((&parsed).into()).ok());
    match rules {
        Some(rules) => {
            for category in rules.categories() {
                categories.insert(match category {
                    PluralCategory::Zero => "zero",
                    PluralCategory::One => "one",
                    PluralCategory::Two => "two",
                    PluralCategory::Few => "few",
                    PluralCategory::Many => "many",
                    PluralCategory::Other => "other",
                });
            }
        }
        None => {
            categories.insert("other");
        }
    }
    let mut guard = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .get_or_insert_with(HashMap::new)
        .insert(locale.to_string(), categories.clone());
    categories
}

/// The one fixed check of person-layer text (section H). `expected_id` is `None` when the entry
/// comes from a drafted reply or an import, where the identifier is read from the entry.
// spec: ordin-desktop/components/Localisation#LOC-LF-2
pub fn check_entry(
    locale: &str,
    expected_id: Option<&str>,
    entry: &str,
) -> Result<(), EntryProblem> {
    // 1. Length and characters.
    let length = entry.chars().count();
    if length == 0 || length > MAX_ENTRY_CHARS {
        return Err(EntryProblem::TooLong);
    }
    if entry.chars().any(is_forbidden) {
        return Err(EntryProblem::ForbiddenCharacter);
    }

    // 2. Exactly one message and nothing else.
    let resource = parser::parse(entry).map_err(|_| EntryProblem::DoesNotParse)?;
    let mut message: Option<&Message<&str>> = None;
    let mut count = 0usize;
    for body in &resource.body {
        match body {
            Entry::Message(found) => {
                count += 1;
                message = Some(found);
            }
            Entry::Term(_) => count += 1,
            Entry::Junk { .. } => return Err(EntryProblem::DoesNotParse),
            Entry::Comment(_) | Entry::GroupComment(_) | Entry::ResourceComment(_) => {}
        }
    }
    let message = match (count, message) {
        (1, Some(message)) => message,
        _ => return Err(EntryProblem::NotOneMessage),
    };

    // 3. Identifier.
    let english = english();
    let id = message.id.name;
    let Some(english_facts) = english.facts.get(id) else {
        return Err(EntryProblem::UnknownMessage);
    };
    if expected_id.is_some_and(|expected| expected != id) {
        return Err(EntryProblem::IdMismatch);
    }

    let facts = facts_of(message);

    // 4. Variables.
    if let Some(name) = english_facts.variables.difference(&facts.variables).next() {
        return Err(EntryProblem::VariableMissing { name: name.clone() });
    }
    if let Some(name) = facts.variables.difference(&english_facts.variables).next() {
        return Err(EntryProblem::VariableUnknown { name: name.clone() });
    }

    // 5. Attributes.
    if facts.attributes != english_facts.attributes || facts.has_value != english_facts.has_value {
        return Err(EntryProblem::AttributeMismatch);
    }

    // 6. References.
    for reference in &facts.references {
        match reference {
            Reference::Function(name) => {
                if name != "NUMBER" && name != "DATETIME" {
                    return Err(EntryProblem::FunctionNotAllowed { name: name.clone() });
                }
            }
            Reference::Message(name) => {
                if !english.facts.contains_key(name) {
                    return Err(EntryProblem::ReferenceUnknown { id: name.clone() });
                }
            }
            Reference::Term(name) => {
                if !english.terms.contains(name) {
                    return Err(EntryProblem::ReferenceUnknown { id: name.clone() });
                }
            }
        }
    }

    // 7. Plural forms.
    let categories = plural_categories(locale);
    for (variable, keys) in &facts.selects {
        let english_keys = english_facts.select_keys.get(variable);
        for key in keys {
            let allowed = categories.contains(key.as_str())
                || english_keys.is_some_and(|set| set.contains(key));
            if !allowed {
                return Err(EntryProblem::PluralCategory { key: key.clone() });
            }
        }
    }
    Ok(())
}
