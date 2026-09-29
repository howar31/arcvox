//! Chat message fitting and the fallback prompt template.
use kongwe_core::types::{ChatMessage, Role};

/// Drops few-shot user/assistant pairs, oldest first, until the messages fit
/// `budget_tokens`. The leading system message and the final message are always
/// kept; when those alone still exceed the budget, only they are returned.
pub fn fit_messages(
    messages: Vec<ChatMessage>,
    budget_tokens: usize,
    count: impl Fn(&str) -> usize,
) -> Vec<ChatMessage> {
    if messages.len() < 2 {
        return messages;
    }
    let mut messages: Vec<(ChatMessage, usize)> = messages
        .into_iter()
        .map(|m| {
            let n = count(&m.content);
            (m, n)
        })
        .collect();
    let first = usize::from(messages[0].0.role == Role::System);
    let mut total: usize = messages.iter().map(|(_, n)| n).sum();
    while total > budget_tokens {
        let last = messages.len() - 1;
        let pair = (first..last.saturating_sub(1)).find(|&i| {
            messages[i].0.role == Role::User && messages[i + 1].0.role == Role::Assistant
        });
        let Some(i) = pair else { break };
        total -= messages[i].1 + messages[i + 1].1;
        messages.drain(i..i + 2);
    }
    if total > budget_tokens {
        let last = messages.pop();
        messages.truncate(first);
        messages.extend(last);
    }
    messages.into_iter().map(|(m, _)| m).collect()
}

/// Renders messages in the ChatML format, for models without a usable template.
pub fn chatml(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        out.push_str("<|im_start|>");
        out.push_str(role_name(m.role));
        out.push('\n');
        out.push_str(&m.content);
        out.push_str("<|im_end|>\n");
    }
    out.push_str("<|im_start|>assistant\n");
    out
}

pub(crate) fn role_name(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msgs() -> Vec<ChatMessage> {
        vec![
            ChatMessage::system("sys"),
            ChatMessage::user("ex1 raw"),
            ChatMessage::assistant("ex1 out"),
            ChatMessage::user("ex2 raw"),
            ChatMessage::assistant("ex2 out"),
            ChatMessage::user("<<< utterance >>>"),
        ]
    }
    fn count(s: &str) -> usize {
        s.split_whitespace().count()
    }

    #[test]
    fn fits_when_under_budget() {
        assert_eq!(fit_messages(msgs(), 100, count).len(), 6);
    }

    #[test]
    fn drops_oldest_few_shot_pair_first() {
        // total = 1 + 2+2 + 2+2 + 2 = 11 tokens; budget 8 drops ex1 (4 tokens)
        let m = fit_messages(msgs(), 8, count);
        assert_eq!(m.len(), 4);
        assert_eq!(m[1].content, "ex2 raw");
    }

    #[test]
    fn keeps_system_and_utterance_when_everything_else_must_go() {
        let m = fit_messages(msgs(), 3, count);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].role, Role::System);
        assert_eq!(m[1].content, "<<< utterance >>>");
    }

    #[test]
    fn chatml_renders_roles() {
        let s = chatml(&[ChatMessage::system("a"), ChatMessage::user("b")]);
        assert_eq!(
            s,
            "<|im_start|>system\na<|im_end|>\n<|im_start|>user\nb<|im_end|>\n<|im_start|>assistant\n"
        );
    }
}
