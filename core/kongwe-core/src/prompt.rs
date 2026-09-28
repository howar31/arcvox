//! Layered prompt: base -> profile addition -> auto-injected context and
//! dictionary -> few-shot examples -> the utterance. The order is fixed.

use crate::config::Example;
use crate::types::{ChatMessage, Context};

pub struct PromptParts<'a> {
    pub base: &'a str,
    pub profile_addition: &'a str,
    pub dictionary_terms: &'a [String],
    pub context: &'a Context,
    pub few_shot: &'a [Example],
    pub utterance: &'a str,
    pub context_limit_bytes: usize,
}

pub fn assemble(parts: &PromptParts<'_>) -> Vec<ChatMessage> {
    let mut system = parts.base.trim_end().to_string();
    if !parts.profile_addition.trim().is_empty() {
        system.push_str("\n\n");
        system.push_str(parts.profile_addition.trim());
    }
    let auto = auto_layer(parts);
    if !auto.is_empty() {
        system.push_str("\n\n");
        system.push_str(&auto);
    }

    let mut messages = vec![ChatMessage::system(system)];
    for ex in parts.few_shot {
        messages.push(ChatMessage::user(ex.raw.clone()));
        messages.push(ChatMessage::assistant(ex.refined.clone()));
    }
    messages.push(ChatMessage::user(parts.utterance.to_string()));
    messages
}

fn auto_layer(parts: &PromptParts<'_>) -> String {
    let limit = parts.context_limit_bytes;
    let mut sections: Vec<String> = Vec::new();
    if !parts.dictionary_terms.is_empty() {
        sections.push(format!(
            "專有名詞（請保留原樣）：{}",
            parts.dictionary_terms.join("、")
        ));
    }
    let ctx = parts.context;
    if let Some(v) = ctx.app_name.as_deref().filter(|s| !s.is_empty()) {
        sections.push(format!("目前應用程式：{}", truncate_utf8(v, limit)));
    }
    if let Some(v) = ctx.window_title.as_deref().filter(|s| !s.is_empty()) {
        sections.push(format!("視窗標題：{}", truncate_utf8(v, limit)));
    }
    if let Some(v) = ctx.selected_text.as_deref().filter(|s| !s.is_empty()) {
        sections.push(format!("使用者選取的文字：\n{}", truncate_utf8(v, limit)));
    }
    if let Some(v) = ctx.clipboard.as_deref().filter(|s| !s.is_empty()) {
        sections.push(format!("剪貼簿內容：\n{}", truncate_utf8(v, limit)));
    }
    sections.join("\n")
}

/// Cuts `s` to at most `max_bytes` without splitting a UTF-8 character.
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Role;

    fn parts<'a>(
        context: &'a Context,
        few_shot: &'a [Example],
        terms: &'a [String],
    ) -> PromptParts<'a> {
        PromptParts {
            base: "BASE",
            profile_addition: "ADD",
            dictionary_terms: terms,
            context,
            few_shot,
            utterance: "原文",
            context_limit_bytes: 4096,
        }
    }

    #[test]
    fn order_is_base_addition_auto_fewshot_utterance() {
        let ctx = Context {
            app_name: Some("Slack".into()),
            ..Default::default()
        };
        let shots = vec![Example {
            raw: "r1".into(),
            refined: "f1".into(),
        }];
        let terms = vec!["Kongwe".to_string()];
        let msgs = assemble(&parts(&ctx, &shots, &terms));
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, Role::System);
        let sys = &msgs[0].content;
        let i_base = sys.find("BASE").unwrap();
        let i_add = sys.find("ADD").unwrap();
        let i_terms = sys.find("Kongwe").unwrap();
        let i_app = sys.find("Slack").unwrap();
        assert!(
            i_base < i_add && i_add < i_terms && i_terms < i_app,
            "{sys}"
        );
        assert_eq!((msgs[1].role, msgs[1].content.as_str()), (Role::User, "r1"));
        assert_eq!(
            (msgs[2].role, msgs[2].content.as_str()),
            (Role::Assistant, "f1")
        );
        assert_eq!(
            (msgs[3].role, msgs[3].content.as_str()),
            (Role::User, "原文")
        );
    }

    #[test]
    fn empty_layers_are_omitted() {
        let ctx = Context::default();
        let msgs = assemble(&PromptParts {
            profile_addition: "  ",
            ..parts(&ctx, &[], &[])
        });
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "BASE");
    }

    #[test]
    fn truncates_on_char_boundary() {
        // Each of these characters is 3 bytes; a 4-byte limit must keep exactly one.
        assert_eq!(truncate_utf8("臺灣正體", 4), "臺");
        assert_eq!(truncate_utf8("臺灣正體", 6), "臺灣");
        assert_eq!(truncate_utf8("abc", 10), "abc");
        assert_eq!(truncate_utf8("臺", 1), "");
        let ctx = Context {
            clipboard: Some("臺灣正體".repeat(100)),
            ..Default::default()
        };
        let msgs = assemble(&PromptParts {
            context_limit_bytes: 7,
            ..parts(&ctx, &[], &[])
        });
        assert!(msgs[0].content.contains("剪貼簿內容：\n臺灣"));
        assert!(!msgs[0].content.contains("臺灣正"));
    }
}
