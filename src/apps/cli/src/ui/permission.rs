/// V2 permission request modal panel.
///
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::string_utils::truncate_str;
use super::theme::{StyleKind, Theme};
use bitfun_agent_runtime::sdk::{PermissionReply, PermissionV2Request};

#[derive(Debug, Clone)]
pub(crate) struct PermissionV2Prompt {
    pub(crate) request: PermissionV2Request,
    pub(crate) selected_option: usize,
    reject_feedback: String,
    editing_reject_feedback: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PermissionV2Action {
    None,
    Reply(PermissionReply),
}

impl PermissionV2Prompt {
    pub(crate) fn new(request: PermissionV2Request) -> Self {
        Self {
            request,
            selected_option: 0,
            reject_feedback: String::new(),
            editing_reject_feedback: false,
        }
    }

    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) -> PermissionV2Action {
        if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
            return PermissionV2Action::None;
        }
        if self.editing_reject_feedback {
            return match (key.code, key.modifiers) {
                (KeyCode::Enter, _) => PermissionV2Action::Reply(PermissionReply::Reject {
                    feedback: match self.reject_feedback.trim() {
                        "" => None,
                        feedback => Some(feedback.to_string()),
                    },
                }),
                (KeyCode::Esc, _) => {
                    self.editing_reject_feedback = false;
                    PermissionV2Action::None
                }
                (KeyCode::Backspace, _) => {
                    self.reject_feedback.pop();
                    PermissionV2Action::None
                }
                (KeyCode::Char(character), KeyModifiers::NONE | KeyModifiers::SHIFT)
                    if !character.is_control() =>
                {
                    self.reject_feedback.push(character);
                    PermissionV2Action::None
                }
                _ => PermissionV2Action::None,
            };
        }
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected_option = self.selected_option.saturating_sub(1);
                PermissionV2Action::None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected_option = (self.selected_option + 1).min(2);
                PermissionV2Action::None
            }
            KeyCode::Esc => PermissionV2Action::Reply(PermissionReply::Reject { feedback: None }),
            KeyCode::Enter => match self.selected_option {
                0 => PermissionV2Action::Reply(PermissionReply::Once),
                1 => PermissionV2Action::Reply(PermissionReply::Always),
                _ => {
                    self.editing_reject_feedback = true;
                    PermissionV2Action::None
                }
            },
            _ => PermissionV2Action::None,
        }
    }
}

// ============ Rendering ============

pub(super) fn render_permission_v2_overlay(
    frame: &mut Frame,
    prompt: &PermissionV2Prompt,
    theme: &Theme,
    area: Rect,
) {
    let overlay_height = 11u16.min(area.height.saturating_sub(2));
    let overlay_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(overlay_height),
        width: area.width,
        height: overlay_height,
    };
    frame.render_widget(Clear, overlay_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(2)])
        .split(overlay_area);
    let content_block = Block::default()
        .borders(Borders::LEFT | Borders::TOP | Borders::RIGHT)
        .border_style(Style::default().fg(theme.warning))
        .style(Style::default().bg(theme.background_panel));
    let inner = content_block.inner(chunks[0]);
    frame.render_widget(content_block, chunks[0]);

    let request = &prompt.request;
    let resources = request
        .resources
        .iter()
        .map(|resource| truncate_str(resource, 80))
        .collect::<Vec<_>>()
        .join(", ");
    let save_scope = if request.save_resources.is_empty() {
        "No remembered scope".to_string()
    } else {
        format!(
            "Always saves {} project resource(s)",
            request.save_resources.len()
        )
    };
    let risk = request
        .display_metadata
        .get("riskDescription")
        .or_else(|| request.display_metadata.get("risk"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("No additional risk information");
    let lines = vec![
        Line::from(Span::styled(
            "Permission required",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Action: {}  Source: {:?}:{}",
            request.action, request.source.kind, request.source.identity
        )),
        Line::from(format!("Resources: {resources}")),
        Line::from(format!("Project: {}  {save_scope}", request.project_id)),
        Line::from(format!("Risk: {}", truncate_str(risk, 100))),
        if prompt.editing_reject_feedback {
            Line::from(format!(
                "Rejection feedback (optional): {}_",
                prompt.reject_feedback
            ))
        } else {
            Line::from("")
        },
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
    render_button_bar(
        frame,
        chunks[1],
        theme,
        if prompt.editing_reject_feedback {
            &["Submit reject"]
        } else {
            &["Allow once", "Always allow", "Reject"]
        },
        if prompt.editing_reject_feedback {
            0
        } else {
            prompt.selected_option
        },
        if prompt.editing_reject_feedback {
            "Enter submit  Esc back"
        } else {
            "\u{21c6} select  Enter confirm  Esc reject"
        },
    );
}

/// Render a horizontal button bar with selectable options
fn render_button_bar(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    options: &[&str],
    selected: usize,
    hint_text: &str,
) {
    let bar_block = Block::default().style(Style::default().bg(theme.background_element));
    frame.render_widget(bar_block, area);

    // Build button spans
    let mut spans = vec![Span::raw(" ")];
    for (i, option) in options.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        if i == selected {
            spans.push(Span::styled(
                format!(" {} ", option),
                Style::default()
                    .fg(theme.background)
                    .bg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", option),
                Style::default()
                    .fg(theme.muted)
                    .bg(theme.background_element),
            ));
        }
    }

    // Add hint text on the right side if there's room
    let buttons_width: usize = spans.iter().map(|s| s.width()).sum();
    let hint_width = hint_text.len() + 2;
    if buttons_width + hint_width < area.width as usize {
        let padding = area.width as usize - buttons_width - hint_width;
        spans.push(Span::raw(" ".repeat(padding)));
        spans.push(Span::styled(hint_text, theme.style(StyleKind::Muted)));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line).style(Style::default().bg(theme.background_element));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::{PermissionV2Action, PermissionV2Prompt};
    use bitfun_agent_runtime::sdk::{
        PermissionReply, PermissionRequestSource, PermissionRequestSourceKind, PermissionV2Request,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use serde_json::Map;

    fn request() -> PermissionV2Request {
        PermissionV2Request {
            request_id: "request-1".to_string(),
            tool_call_id: None,
            project_id: "project-1".to_string(),
            session_id: "session-1".to_string(),
            agent_id: "agentic".to_string(),
            action: "edit".to_string(),
            resources: vec!["src/main.rs".to_string()],
            save_resources: vec!["src/main.rs".to_string()],
            source: PermissionRequestSource {
                kind: PermissionRequestSourceKind::ToolCall,
                identity: "write_file".to_string(),
            },
            display_metadata: Map::new(),
        }
    }

    #[test]
    fn v2_prompt_returns_project_always_reply_without_using_legacy_runtime_scope() {
        let mut prompt = PermissionV2Prompt::new(request());
        prompt.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        assert_eq!(
            prompt.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PermissionV2Action::Reply(PermissionReply::Always)
        );
    }

    #[test]
    fn v2_prompt_collects_optional_rejection_feedback() {
        let mut prompt = PermissionV2Prompt::new(request());
        prompt.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        prompt.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            prompt.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PermissionV2Action::None
        );
        for character in "read only".chars() {
            prompt.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }

        assert_eq!(
            prompt.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            PermissionV2Action::Reply(PermissionReply::Reject {
                feedback: Some("read only".to_string()),
            })
        );
    }
}
