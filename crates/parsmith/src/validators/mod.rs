//! Pure-Rust compile-time validators for CSS3/4 selectors, XPath 1.0 expressions, and regular expressions (`E002`).

pub mod css;
pub mod css_to_xpath;
pub mod regex;
pub mod xpath;

pub use css::validate_css_selector;
pub use css_to_xpath::{css_to_xpath, xpath_literal};
pub use regex::{extract_inline_flags, normalize_regex_pattern, unverbosify_regex, validate_regex};
pub use xpath::validate_xpath;
