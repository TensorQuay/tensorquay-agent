//! Fixed prompt slots followed by the entire chronological history.
use crate::{ContextError, Message, Role, message_tokens};

/// Explicit input-window accounting; guesses use only three quarters of the supplied window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budget {
    /// The route's total context capacity.
    pub context_window: u32,
    /// Capacity reserved for the next output.
    pub reserve_output: u32,
    /// Whether capacity was guessed rather than derived.
    pub guessed: bool,
}

impl Budget {
    /// Input tokens available: the window (three quarters of it when guessed) minus the
    /// output reserve.
    #[must_use]
    pub fn input_capacity(&self) -> u32 {
        let window = if self.guessed {
            self.context_window / 4 * 3 + self.context_window % 4 * 3 / 4
        } else {
            self.context_window
        };
        window.saturating_sub(self.reserve_output)
    }
}

fn slots(system: &str, instructions: &str, tools_json: &str, history: &[Message]) -> Vec<Message> {
    let mut messages: Vec<_> = [system, instructions, tools_json]
        .into_iter()
        .map(|text| Message::new(Role::System, text))
        .collect();
    messages.extend_from_slice(history);
    messages
}

fn total_tokens(messages: &[Message]) -> u64 {
    messages
        .iter()
        .map(|message| u64::from(message_tokens(message).tokens))
        .fold(0_u64, u64::saturating_add)
}

/// A complete request whose fixed prefix and history fit the supplied budget.
#[derive(Clone, Debug)]
pub struct Window {
    messages: Vec<Message>,
    prefix: Vec<u8>,
}
impl Window {
    /// Builds system, instruction and tool slots, then history in its original order.
    ///
    /// # Errors
    /// Returns `OverBudget` if the whole request does not fit; never discards history.
    pub fn assemble(
        system: &str,
        instructions: &str,
        tools_json: &str,
        history: &[Message],
        budget: Budget,
    ) -> Result<Self, ContextError> {
        Self::build(
            [system, instructions, tools_json],
            tools_json,
            history,
            budget,
        )
    }
    /// Like [`Window::assemble`] for a route with native tool calls, where the catalogue travels in the request's
    /// `tools`: the tools slot is empty, but the catalogue is still counted and still part of the cache prefix, since
    /// the provider reads it all the same (CTX-U-017).
    ///
    /// # Errors
    /// Returns `OverBudget` if the whole request, catalogue included, does not fit; never discards history.
    pub fn assemble_native(
        system: &str,
        instructions: &str,
        tools_json: &str,
        history: &[Message],
        budget: Budget,
    ) -> Result<Self, ContextError> {
        Self::build([system, instructions, ""], tools_json, history, budget)
    }
    /// The window with `sent` as its three slots; the budget and the prefix count the catalogue `tools_json`.
    fn build(
        sent: [&str; 3],
        tools_json: &str,
        history: &[Message],
        budget: Budget,
    ) -> Result<Self, ContextError> {
        let [system, instructions, slot] = sent;
        let available = budget.input_capacity();
        let needed = total_tokens(&slots(system, instructions, tools_json, history));
        if needed > u64::from(available) {
            return Err(ContextError::OverBudget { needed, available });
        }
        let mut prefix = Vec::new();
        for slot in [system, instructions, tools_json] {
            prefix.extend_from_slice(&(slot.len() as u64).to_be_bytes());
            prefix.extend_from_slice(slot.as_bytes());
        }
        let messages = slots(system, instructions, slot, history);
        Ok(Self { messages, prefix })
    }
    /// Estimates the tokens [`Window::assemble`] (or [`Window::assemble_native`]) would count for the same request.
    #[must_use]
    pub fn needed_tokens(
        system: &str,
        instructions: &str,
        tools_json: &str,
        history: &[Message],
    ) -> u64 {
        total_tokens(&slots(system, instructions, tools_json, history))
    }
    /// Returns the fixed slots and history, in provider-request order.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }
    /// Returns three length-delimited UTF-8 slots for unambiguous prefix-stability checks.
    #[must_use]
    pub fn prefix_bytes(&self) -> &[u8] {
        &self.prefix
    }
}
