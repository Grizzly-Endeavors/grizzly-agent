//! The system prompt as an ordered list of sections, rendered fresh before
//! every model call.

use crate::message::Message;

/// A system-prompt section whose text can change between model calls.
///
/// Implementations hold whatever state they render from — an index, active
/// skills, anything a consumer wants visible mid-run — and are rendered
/// fresh before every call, so a change made by a tool earlier in the run is
/// visible on the next round. This is the seam a consumer uses to inject
/// context that changes mid-run.
pub trait DynamicSection: Send + Sync {
    /// Renders this section's current text.
    ///
    /// An empty or whitespace-only result drops the section from that
    /// call's system prompt entirely.
    fn render(&self) -> String;
}

/// One entry in an [`crate::Agent`]'s system prompt: fixed text, or a
/// [`DynamicSection`] rendered fresh before every call.
pub enum SystemSection {
    /// Fixed text, rendered unchanged on every call.
    Static(String),
    /// Text rendered fresh from a [`DynamicSection`] before every call.
    Dynamic(Box<dyn DynamicSection>),
}

impl SystemSection {
    /// A section rendered fresh from `section` before every model call.
    #[must_use]
    pub fn dynamic(section: impl DynamicSection + 'static) -> Self {
        Self::Dynamic(Box::new(section))
    }

    fn render(&self) -> String {
        match self {
            Self::Static(text) => text.clone(),
            Self::Dynamic(section) => section.render(),
        }
    }
}

impl From<String> for SystemSection {
    fn from(text: String) -> Self {
        Self::Static(text)
    }
}

impl From<&str> for SystemSection {
    fn from(text: &str) -> Self {
        Self::Static(text.to_owned())
    }
}

/// Renders every section in list order, drops any whose rendering is empty
/// or whitespace-only, and joins the rest with a blank line into one system
/// message at the head of the request — or `None` if every section rendered
/// empty.
pub(crate) fn render_system_prompt(sections: &[SystemSection]) -> Option<Message> {
    let joined = sections
        .iter()
        .map(SystemSection::render)
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    (!joined.is_empty()).then(|| Message::system(joined))
}

#[cfg(test)]
#[path = "tests/section.rs"]
mod tests;
