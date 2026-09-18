//! Tests for [`super`].

use super::{DynamicSection, SystemSection, render_system_prompt};
use crate::message::{Content, Role};

struct Fixed(&'static str);

impl DynamicSection for Fixed {
    fn render(&self) -> String {
        self.0.to_owned()
    }
}

struct Blank;

impl DynamicSection for Blank {
    fn render(&self) -> String {
        "   \n  ".to_owned()
    }
}

#[test]
fn empty_sections_are_dropped_and_the_rest_joined_with_a_blank_line() {
    let sections = vec![
        SystemSection::from("first section"),
        SystemSection::from(""),
        SystemSection::dynamic(Blank),
        SystemSection::dynamic(Fixed("second section")),
    ];

    let message = render_system_prompt(&sections).expect("at least one section has text");

    assert_eq!(
        message.role,
        Role::System,
        "sections render into a system message"
    );
    assert_eq!(
        message.content,
        vec![Content::Text("first section\n\nsecond section".to_owned())],
        "empty and whitespace-only sections must not appear, and survivors join with a blank line"
    );
}

#[test]
fn all_sections_empty_renders_no_system_message() {
    let sections = vec![
        SystemSection::from(""),
        SystemSection::dynamic(Blank),
        SystemSection::from("   "),
    ];

    assert!(
        render_system_prompt(&sections).is_none(),
        "a system prompt with no non-empty section must not produce a message at all"
    );
}

#[test]
fn a_dynamic_section_renders_fresh_each_call() {
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Counter(AtomicU32);

    impl DynamicSection for Counter {
        fn render(&self) -> String {
            let value = self.0.fetch_add(1, Ordering::SeqCst);
            format!("call {value}")
        }
    }

    let sections = vec![SystemSection::dynamic(Counter(AtomicU32::new(0)))];

    let first = render_system_prompt(&sections).expect("non-empty render");
    let second = render_system_prompt(&sections).expect("non-empty render");

    assert_ne!(
        first, second,
        "rendering again must call the section again, not reuse a cached result"
    );
}

#[test]
fn no_sections_at_all_renders_no_system_message() {
    assert!(
        render_system_prompt(&[]).is_none(),
        "an Agent with no sections must not send an empty system message"
    );
}
