//! ICU4X-backed formatting and the Fluent bundles (Localisation.md Internal Logic B1 and G).

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{Datelike, Local, Timelike};
use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use icu::collator::options::CollatorOptions;
use icu::collator::{Collator, CollatorBorrowed};
use icu::datetime::fieldsets::{self, YMD};
use icu::datetime::input::{Date, DateTime, Time};
use icu::datetime::DateTimeFormatter;
use icu::decimal::input::Decimal;
use icu::decimal::DecimalFormatter;
use icu::list::options::ListFormatterOptions;
use icu::list::ListFormatter;
use icu::locale::{Locale, LocaleDirectionality};
use intl_memoizer::concurrent::IntlLangMemoizer;
use intl_memoizer::Memoizable;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use unic_langid::LanguageIdentifier;

use crate::types::{DateTimeStyle, TextDirection};

/// One Fluent bundle (concurrent; the resources are shared between rebuilds).
pub(crate) type Bundle = FluentBundle<Arc<FluentResource>>;

/// Every ICU4X formatter of one locale, built once and shared.
pub(crate) struct Formatters {
    decimal: DecimalFormatter,
    list: ListFormatter,
    collator: CollatorBorrowed<'static>,
    date: DateTimeFormatter<YMD>,
    short_date: DateTimeFormatter<YMD>,
    time: DateTimeFormatter<fieldsets::T>,
    date_time: DateTimeFormatter<fieldsets::YMDT>,
}

impl Formatters {
    /// Builds the formatters of `tag`. A formatter ICU4X cannot build for the tag is built for the
    /// root locale instead, so formatting never fails.
    pub(crate) fn new(tag: &str) -> Formatters {
        let locale = Locale::try_from_str(tag).unwrap_or(Locale::UNKNOWN);
        Formatters {
            decimal: or_root(&locale, |l| {
                DecimalFormatter::try_new(l.into(), Default::default())
            }),
            list: or_root(&locale, |l| {
                ListFormatter::try_new_and(l.into(), ListFormatterOptions::default())
            }),
            collator: or_root(&locale, |l| {
                Collator::try_new(l.into(), CollatorOptions::default())
            }),
            date: or_root(&locale, |l| {
                DateTimeFormatter::try_new(l.into(), YMD::medium())
            }),
            short_date: or_root(&locale, |l| {
                DateTimeFormatter::try_new(l.into(), YMD::short())
            }),
            time: or_root(&locale, |l| {
                DateTimeFormatter::try_new(l.into(), fieldsets::T::medium())
            }),
            date_time: or_root(&locale, |l| {
                DateTimeFormatter::try_new(l.into(), YMD::medium().with_time_hm())
            }),
        }
    }

    /// A decimal string formatted with the locale's number rules; the text unchanged when it is not
    /// a decimal.
    pub(crate) fn number_text(&self, text: &str) -> String {
        match Decimal::try_from_str(text) {
            Ok(decimal) => self.decimal.format(&decimal).to_string(),
            Err(_) => text.to_string(),
        }
    }

    pub(crate) fn list(&self, items: &[String]) -> String {
        self.list.format_to_string(items.iter().map(String::as_str))
    }

    pub(crate) fn compare(&self, a: &str, b: &str) -> std::cmp::Ordering {
        self.collator.compare(a, b)
    }

    /// `at` in the computer's local offset, formatted by the style's field set; no zone name.
    pub(crate) fn datetime(&self, at: Timestamp, style: DateTimeStyle) -> String {
        let local = at.0.with_timezone(&Local);
        let date = u8::try_from(local.month())
            .ok()
            .zip(u8::try_from(local.day()).ok())
            .and_then(|(month, day)| Date::try_new_iso(local.year(), month, day).ok());
        let time = u8::try_from(local.hour())
            .ok()
            .zip(u8::try_from(local.minute()).ok())
            .zip(u8::try_from(local.second()).ok())
            .and_then(|((h, m), s)| Time::try_new(h, m, s, 0).ok());
        let (Some(date), Some(time)) = (date, time) else {
            return at.to_wire();
        };
        match style {
            DateTimeStyle::Date => self.date.format(&date).to_string(),
            DateTimeStyle::ShortDate => self.short_date.format(&date).to_string(),
            DateTimeStyle::Time => self.time.format(&time).to_string(),
            DateTimeStyle::DateTime => self.date_time.format(&DateTime { date, time }).to_string(),
        }
    }
}

/// Builds with `locale`, else with the root locale.
fn or_root<T, E>(locale: &Locale, make: impl Fn(&Locale) -> Result<T, E>) -> T {
    match make(locale) {
        Ok(built) => built,
        Err(_) => make(&Locale::UNKNOWN).unwrap_or_else(|_| unreachable_root()),
    }
}

#[cold]
fn unreachable_root() -> ! {
    // ICU4X compiled data always holds the root locale; reaching this is a build defect.
    std::process::abort()
}

/// The shared formatter set per locale tag.
#[derive(Default)]
pub(crate) struct FormatterCache {
    map: Mutex<HashMap<String, Arc<Formatters>>>,
}

impl FormatterCache {
    pub(crate) fn get(&self, tag: &str) -> Arc<Formatters> {
        let mut map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            map.entry(tag.to_string())
                .or_insert_with(|| Arc::new(Formatters::new(tag))),
        )
    }
}

/// `ar-XB` is right-to-left by decision; otherwise the CLDR script of the tag decides.
pub(crate) fn direction_of(tag: &str) -> TextDirection {
    if tag == "ar-XB" {
        return TextDirection::Rtl;
    }
    match Locale::try_from_str(tag) {
        Ok(locale) if LocaleDirectionality::new_common().is_right_to_left(&locale.id) => {
            TextDirection::Rtl
        }
        _ => TextDirection::Ltr,
    }
}

/// The memoised decimal formatter fluent-bundle reaches through its number-formatting hook.
struct IcuDecimal(DecimalFormatter);

impl Memoizable for IcuDecimal {
    type Args = ();
    type Error = ();

    fn construct(lang: LanguageIdentifier, _args: Self::Args) -> Result<Self, Self::Error> {
        let locale = Locale::try_from_str(&lang.to_string()).unwrap_or(Locale::UNKNOWN);
        DecimalFormatter::try_new((&locale).into(), Default::default())
            .map(IcuDecimal)
            .map_err(|_| ())
    }
}

/// Formats a Fluent number with the bundle locale's CLDR decimal rules (SA-I18N-02).
fn number_hook(value: &FluentValue, intls: &IntlLangMemoizer) -> Option<String> {
    let FluentValue::Number(number) = value else {
        return None;
    };
    let text = number.value.to_string();
    intls
        .with_try_get::<IcuDecimal, _, _>((), |formatter| {
            Decimal::try_from_str(&text)
                .ok()
                .map(|decimal| formatter.0.format(&decimal).to_string())
        })
        .ok()
        .flatten()
}

/// A bundle for `tag` with `NUMBER` and `DATETIME` backed by ICU4X, isolating on.
pub(crate) fn new_bundle(tag: &str, formatters: Arc<Formatters>) -> Bundle {
    let language: LanguageIdentifier = tag.parse().unwrap_or_default();
    let mut bundle = Bundle::new_concurrent(vec![language]);
    bundle.set_use_isolating(true);
    bundle.set_formatter(Some(number_hook));

    let number_formatters = Arc::clone(&formatters);
    // Registering on an empty bundle cannot fail: the names are unused.
    let _ = bundle.add_function("NUMBER", move |positional, _named| {
        match positional.first() {
            Some(FluentValue::Number(number)) => FluentValue::Number(number.clone()),
            Some(FluentValue::String(text)) => {
                FluentValue::String(Cow::Owned(number_formatters.number_text(text)))
            }
            _ => FluentValue::Error,
        }
    });
    let date_formatters = formatters;
    let _ = bundle.add_function("DATETIME", move |positional, named: &FluentArgs| {
        let Some(FluentValue::String(text)) = positional.first() else {
            return FluentValue::Error;
        };
        let Ok(parsed) = parse_rfc3339_ms_utc(text) else {
            return FluentValue::Error;
        };
        let style = match named.get("style") {
            Some(FluentValue::String(style)) => match style.as_ref() {
                "date" => DateTimeStyle::Date,
                "time" => DateTimeStyle::Time,
                "shortdate" => DateTimeStyle::ShortDate,
                _ => DateTimeStyle::DateTime,
            },
            _ => DateTimeStyle::DateTime,
        };
        FluentValue::String(Cow::Owned(
            date_formatters.datetime(Timestamp(parsed), style),
        ))
    });
    bundle
}
