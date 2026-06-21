use std::{sync::Arc, time::Duration};

use lurq::{
  animation::Transition,
  app::{
    component::{Component, DevtoolsInspectable},
    ctx::{Ctx, Timeout},
    events::KeyboardEvent,
  },
  components::{Column, Row, ScrollVertical, Text, TextInput},
  core::{ElementRef, Signal},
  layout::{
    Alignment,
    layout_kind::{Justify, ScrollState},
  },
  node::{BackgroundColor, CursorIcon, Element, Style, color::Color, dimension::Dimension, transform::Transform2D},
};

use super::{
  super::{SendChatAction, SendChatInput},
  channel::ChatChannel,
  scroll::chat_scrollbar_style,
};
use crate::{
  network::protocol::{
    ChannelId,
    control::{ChatCommandQueryResponse, ChatCommandQueryResult, ChatCommandQueryStatus},
  },
  session::chat_commands::{ChatCommandRegistry, CommandDefinition},
  theme,
  ui::common::lucide_icon::{LucideIcon, LucideIconProps},
};

pub(super) const CHAT_COMMAND_SUGGESTION_BOTTOM_GAP: f32 = 6.0;
const CHAT_COMMAND_SUGGESTION_ROW_HEIGHT: f32 = 68.0;
const CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT: f32 = 32.0;
const CHAT_COMMAND_INVALID_SHAKE_STEP_MS: u64 = 28;
const CHAT_COMMAND_INVALID_SHAKE_TRANSITION_MS: u64 = 20;

#[derive(Clone)]
pub(in crate::ui::lobby) struct ChatCommandInvalidFeedback {
  phase: Signal<u8>,
  step_two: Timeout,
  step_three: Timeout,
  step_four: Timeout,
  step_five: Timeout,
  reset: Timeout,
}

impl ChatCommandInvalidFeedback {
  pub(in crate::ui::lobby) fn new(ctx: &mut Ctx) -> Self {
    let phase = ctx.signal(0_u8);
    let step_two_phase = phase.clone();
    let step_three_phase = phase.clone();
    let step_four_phase = phase.clone();
    let step_five_phase = phase.clone();
    let reset_phase = phase.clone();

    Self {
      phase,
      step_two: ctx.create_timeout(Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS), move || {
        step_two_phase.set(2);
      }),
      step_three: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 2),
        move || {
          step_three_phase.set(3);
        },
      ),
      step_four: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 3),
        move || {
          step_four_phase.set(4);
        },
      ),
      step_five: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 4),
        move || {
          step_five_phase.set(5);
        },
      ),
      reset: ctx.create_timeout(
        Duration::from_millis(CHAT_COMMAND_INVALID_SHAKE_STEP_MS * 5),
        move || {
          reset_phase.set(0);
        },
      ),
    }
  }

  fn phase(&self) -> u8 {
    self.phase.get()
  }

  fn trigger(&self) {
    self.phase.set(1);
    self.step_two.restart();
    self.step_three.restart();
    self.step_four.restart();
    self.step_five.restart();
    self.reset.restart();
  }
}

pub(super) fn chat_composer(
  ctx: &mut Ctx,
  channel: &ChatChannel,
  message_input: Signal<String>,
  command_selected_index: Signal<usize>,
  command_invalid_feedback: ChatCommandInvalidFeedback,
  command_registry: ChatCommandRegistry,
  command_query_response: Option<ChatCommandQueryResponse>,
  command_query_request_id: u64,
  send_chat: &SendChatAction,
  composer_ref: ElementRef,
) -> Element {
  let text_style = ctx.theme().typography().description.clone();
  let mut placeholder_style = text_style.clone();
  placeholder_style.color = theme::palette().text_muted.with_opacity(0.65);
  let placeholder = ctx.t_args(
    "lobby.text_channel.composer_placeholder",
    [("channel", channel.name().to_owned())],
  );
  let channel_id = channel.server_channel_id();
  let key_value = message_input.clone();
  let key_command_selected_index = command_selected_index.clone();
  let key_invalid_feedback = command_invalid_feedback.clone();
  let key_command_registry = command_registry.clone();
  let key_command_query_response = command_query_response.clone();
  let key_action = send_chat.clone();
  let click_value = message_input.clone();
  let click_command_selected_index = command_selected_index.clone();
  let click_invalid_feedback = command_invalid_feedback.clone();
  let click_command_registry = command_registry.clone();
  let click_action = send_chat.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .ref_element(composer_ref)
    .padding_left(24.0)
    .padding_right(24.0)
    .padding_bottom(theme::SpacingSize::Xl)
    .child(
      Row::new()
        .width(Dimension::Pct(100.0))
        .height(64.0)
        .align_items(Alignment::Center)
        .spacing(theme::SpacingSize::Md)
        .padding_vertical(8.0)
        .padding_left(theme::SpacingSize::Lg)
        .padding_right(theme::SpacingSize::Sm)
        .rounded(theme::RadiusSize::Lg)
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceInput))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(
          TextInput::styled(message_input, text_style)
            .placeholder(&placeholder)
            .placeholder_style(placeholder_style)
            .multiline()
            .name("lobby-chat-message")
            .height(Dimension::Pct(100.0))
            .flex(1.0)
            .background(BackgroundColor::Color(Color::from_hex("#00000000")))
            .caret_color(theme::PaletteColor::Accent)
            .on_key_down(move |event: KeyboardEvent| {
              if handle_chat_command_navigation(
                &key_command_registry,
                &key_value,
                &key_command_selected_index,
                key_command_query_response.as_ref(),
                command_query_request_id,
                &event.key,
                &event.code,
              ) {
                event.prevent_default();
                return;
              }
              if event.key == "Enter" && !event.shift {
                event.prevent_default();
                submit_chat_if_valid(
                  channel_id,
                  &key_value,
                  &key_command_selected_index,
                  &key_invalid_feedback,
                  &key_command_registry,
                  &key_action,
                );
              }
            }),
        )
        .child(
          Row::new()
            .width(32.0)
            .height(32.0)
            .align_items(Alignment::Center)
            .justify(Justify::Center)
            .rounded(theme::RadiusSize::Md)
            .background(BackgroundColor::Palette(theme::PaletteColor::Accent))
            .cursor(CursorIcon::Pointer)
            .hovered_style(Style::new().background(BackgroundColor::Palette(theme::PaletteColor::AccentHover)))
            .on_click(move |_| {
              submit_chat_if_valid(
                channel_id,
                &click_value,
                &click_command_selected_index,
                &click_invalid_feedback,
                &click_command_registry,
                &click_action,
              )
            })
            .child(ctx.mount::<LucideIcon>(LucideIconProps {
              icon: "send-horizontal",
              size: 15.0,
              color: theme::palette().text_inverse,
            })),
        ),
    )
    .into()
}

fn handle_chat_command_navigation(
  command_registry: &ChatCommandRegistry,
  message_input: &Signal<String>,
  selected_index: &Signal<usize>,
  command_query_response: Option<&ChatCommandQueryResponse>,
  command_query_request_id: u64,
  key: &str,
  code: &str,
) -> bool {
  if !command_registry.has_commands() {
    return false;
  }

  let is_tab_completion = matches!((key, code), ("Tab", _) | (_, "Tab"));
  let is_enter_completion = matches!((key, code), ("Enter", _) | (_, "Enter"));
  if let Some(results) = active_chat_command_query_results(
    command_registry,
    &message_input.get_untracked(),
    command_query_response,
    command_query_request_id,
  ) {
    if results.is_empty() {
      selected_index.set(0);
      return false;
    }
    if is_tab_completion {
      return true;
    }
    if is_enter_completion {
      let result = &results[selected_index.get_untracked().min(results.len().saturating_sub(1))];
      fill_chat_command_query_result(command_registry, message_input, result);
      selected_index.set(0);
      return true;
    }

    let direction = match (key, code) {
      ("ArrowDown", _) | (_, "ArrowDown") => 1,
      ("ArrowUp", _) | (_, "ArrowUp") => -1,
      _ => return false,
    };
    let current = selected_index.get_untracked().min(results.len() - 1);
    let next = if direction > 0 {
      (current + 1) % results.len()
    } else {
      current.checked_sub(1).unwrap_or(results.len() - 1)
    };
    selected_index.set(next);
    return true;
  }

  if is_tab_completion || is_enter_completion {
    let input = message_input.get_untracked();
    let Some(query) = command_suggestion_query(&input) else {
      selected_index.set(0);
      return false;
    };
    let commands = matching_chat_command_definitions(command_registry, &query);
    let Some(command) = commands.get(selected_index.get_untracked().min(commands.len().saturating_sub(1))) else {
      selected_index.set(0);
      return false;
    };
    if is_enter_completion && command.name.to_ascii_lowercase() == query {
      return false;
    }
    message_input.set(command_fill_text(command.name.as_ref()));
    return true;
  }

  let direction = match (key, code) {
    ("ArrowDown", _) | (_, "ArrowDown") => 1,
    ("ArrowUp", _) | (_, "ArrowUp") => -1,
    _ => return false,
  };
  let input = message_input.get_untracked();
  let Some(query) = command_suggestion_query(&input) else {
    selected_index.set(0);
    return false;
  };
  let count = matching_chat_command_definitions(command_registry, &query).len();
  if count == 0 {
    selected_index.set(0);
    return false;
  }

  let current = selected_index.get_untracked().min(count - 1);
  let next = if direction > 0 {
    (current + 1) % count
  } else {
    current.checked_sub(1).unwrap_or(count - 1)
  };
  selected_index.set(next);
  true
}

fn command_suggestion_query(input: &str) -> Option<String> {
  let trimmed = input.trim_start();
  if !trimmed.starts_with('/') {
    return None;
  }
  Some(
    trimmed
      .split_whitespace()
      .next()
      .unwrap_or(trimmed)
      .to_ascii_lowercase(),
  )
}

fn matching_chat_command_definitions<'a>(
  command_registry: &'a ChatCommandRegistry,
  query: &str,
) -> Vec<&'a CommandDefinition> {
  command_registry
    .definitions()
    .iter()
    .filter(|command| command.name.to_ascii_lowercase().starts_with(query))
    .collect()
}

fn exact_chat_command_definition<'a>(
  command_registry: &'a ChatCommandRegistry,
  input: &str,
) -> Option<&'a CommandDefinition> {
  let command_name = input.trim_start().split_whitespace().next()?;
  command_registry
    .definitions()
    .iter()
    .find(|command| command.name.as_ref() == command_name)
}

pub(super) fn chat_command_suggestions(
  ctx: &mut Ctx,
  message_input: Signal<String>,
  selected_index: Signal<usize>,
  command_registry: &ChatCommandRegistry,
  scroll_state: ScrollState,
  invalid_feedback: ChatCommandInvalidFeedback,
  command_query_response: Option<ChatCommandQueryResponse>,
  command_query_request_id: u64,
) -> Option<Element> {
  let input = message_input.get();
  if command_registry.live_query_for_input(&input).is_some() {
    return chat_command_query_suggestions(
      ctx,
      message_input,
      selected_index,
      command_registry,
      scroll_state,
      command_query_response.as_ref(),
      command_query_request_id,
    );
  }

  let query = command_suggestion_query(&input)?;
  let commands = matching_chat_command_definitions(command_registry, &query);
  if commands.is_empty() {
    selected_index.set(0);
    return None;
  }

  let active_index = selected_index.get().min(commands.len().saturating_sub(1));
  if active_index != selected_index.get_untracked() {
    selected_index.set(active_index);
  }

  let list_height = command_suggestion_list_height(commands.len());
  ensure_command_selection_visible(&scroll_state, active_index, list_height);
  let invalid_feedback_phase = invalid_feedback.phase();
  let exact_command_name = exact_chat_command_definition(command_registry, &input).map(|command| command.name.as_ref());
  let suggestions_height = CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT + list_height + CHAT_COMMAND_SUGGESTION_BOTTOM_GAP;

  let title = if query == "/" {
    ctx.t("lobby.text_channel.commands.title").to_string()
  } else {
    ctx
      .t_args(
        "lobby.text_channel.commands.matching",
        [("query", query.to_ascii_uppercase())],
      )
      .to_string()
  };

  let rows = commands
    .into_iter()
    .enumerate()
    .map(|(index, command)| {
      let validate_arguments = invalid_feedback_phase != 0 && exact_command_name == Some(command.name.as_ref());
      let usage_parts = command_usage_parts(command.usage.as_ref(), &input, validate_arguments);
      let row_invalid_feedback_phase = if usage_parts
        .iter()
        .any(|part| matches!(part, CommandUsagePart::Argument { invalid: true, .. }))
      {
        invalid_feedback_phase
      } else {
        0
      };
      CommandSuggestionRowProps {
        fill: command_fill_text(command.name.as_ref()),
        description: command_description(ctx, command),
        usage_parts,
        message_input: message_input.clone(),
        selected_index: selected_index.clone(),
        index,
        selected: index == active_index,
        invalid_feedback_phase: row_invalid_feedback_phase,
      }
    })
    .collect::<Vec<_>>();
  let list = Column::new()
    .width(Dimension::Pct(100.0))
    .padding_bottom(6.0)
    .with_children(ctx.for_each(
      rows,
      |row| row.fill.clone(),
      |ctx, row| {
        let key = row.fill.clone();
        ctx.mount_keyed::<CommandSuggestionRow>(&key, row)
      },
    ));

  Some(
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(suggestions_height)
      .padding_left(24.0)
      .padding_right(24.0)
      .padding_bottom(CHAT_COMMAND_SUGGESTION_BOTTOM_GAP)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .rounded(theme::RadiusSize::Lg)
          .clip()
          .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
          .border_inside(1.0, theme::PaletteColor::Border)
          .child(command_suggestion_title(&title))
          .child(
            ScrollVertical::new(list)
              .width(Dimension::Pct(100.0))
              .height(list_height)
              .with_scroll_state(scroll_state)
              .scrollbar(chat_scrollbar_style()),
          ),
      )
      .into(),
  )
}

fn chat_command_query_suggestions(
  ctx: &mut Ctx,
  message_input: Signal<String>,
  selected_index: Signal<usize>,
  command_registry: &ChatCommandRegistry,
  scroll_state: ScrollState,
  response: Option<&ChatCommandQueryResponse>,
  command_query_request_id: u64,
) -> Option<Element> {
  let input = message_input.get();
  let query = command_registry.live_query_for_input(&input)?;
  if query.query.len() < query.input.min_chars as usize {
    selected_index.set(0);
    let title = if query.input.placeholder.trim().is_empty() {
      "Search".to_owned()
    } else {
      query.input.placeholder.to_string()
    };
    let message = if query.input.min_chars > 1 {
      format!("Type at least {} characters", query.input.min_chars)
    } else {
      title.clone()
    };
    return Some(command_query_message_panel(&title, &message));
  }

  let response = active_chat_command_query_response(command_registry, &input, response, command_query_request_id)?;
  let title = match response.status {
    ChatCommandQueryStatus::Ok => "Results".to_owned(),
    _ if !response.message.trim().is_empty() => response.message.clone(),
    _ => "No results".to_owned(),
  };

  if response.results.is_empty() {
    selected_index.set(0);
    return Some(command_query_message_panel(&title, &title));
  }

  let active_index = selected_index.get().min(response.results.len().saturating_sub(1));
  if active_index != selected_index.get_untracked() {
    selected_index.set(active_index);
  }

  let list_height = command_suggestion_list_height(response.results.len());
  ensure_command_selection_visible(&scroll_state, active_index, list_height);
  let rows = response
    .results
    .iter()
    .cloned()
    .enumerate()
    .map(|(index, result)| CommandQueryResultRowProps {
      result,
      message_input: message_input.clone(),
      command_registry: command_registry.clone(),
      selected_index: selected_index.clone(),
      index,
      selected: index == active_index,
    })
    .collect::<Vec<_>>();
  let list = Column::new()
    .width(Dimension::Pct(100.0))
    .padding_bottom(6.0)
    .with_children(ctx.for_each(
      rows,
      |row| row.result.id.clone(),
      |ctx, row| {
        let key = row.result.id.clone();
        ctx.mount_keyed::<CommandQueryResultRow>(&key, row)
      },
    ));

  Some(command_query_panel(
    command_suggestion_title(&title),
    ScrollVertical::new(list)
      .width(Dimension::Pct(100.0))
      .height(list_height)
      .with_scroll_state(scroll_state)
      .scrollbar(chat_scrollbar_style())
      .into(),
    list_height,
  ))
}

fn command_query_message_panel(title: &str, message: &str) -> Element {
  command_query_panel(
    command_suggestion_title(title),
    Column::new()
      .width(Dimension::Pct(100.0))
      .height(CHAT_COMMAND_SUGGESTION_ROW_HEIGHT)
      .align_items(Alignment::Center)
      .padding_horizontal(theme::SpacingSize::Lg)
      .child(
        Text::new(message)
          .variant(theme::TypographyStyle::Link)
          .color(theme::PaletteColor::TextSecondary)
          .nowrap(),
      )
      .into(),
    CHAT_COMMAND_SUGGESTION_ROW_HEIGHT + 6.0,
  )
}

fn command_query_panel(title: Element, content: Element, content_height: f32) -> Element {
  let suggestions_height = CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT + content_height + CHAT_COMMAND_SUGGESTION_BOTTOM_GAP;
  Column::new()
    .width(Dimension::Pct(100.0))
    .height(suggestions_height)
    .padding_left(24.0)
    .padding_right(24.0)
    .padding_bottom(CHAT_COMMAND_SUGGESTION_BOTTOM_GAP)
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .rounded(theme::RadiusSize::Lg)
        .clip()
        .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
        .border_inside(1.0, theme::PaletteColor::Border)
        .child(title)
        .child(content),
    )
    .into()
}

fn active_chat_command_query_response<'a>(
  command_registry: &ChatCommandRegistry,
  input: &str,
  response: Option<&'a ChatCommandQueryResponse>,
  _command_query_request_id: u64,
) -> Option<&'a ChatCommandQueryResponse> {
  let query = command_registry.live_query_for_input(input)?;
  if query.query.len() < query.input.min_chars as usize {
    return None;
  }
  let response = response?;
  (response.command_name == query.command_name && response.argument_name == query.argument_name)
    .then_some(response)
}

fn active_chat_command_query_results<'a>(
  command_registry: &ChatCommandRegistry,
  input: &str,
  response: Option<&'a ChatCommandQueryResponse>,
  command_query_request_id: u64,
) -> Option<&'a [ChatCommandQueryResult]> {
  let response = active_chat_command_query_response(command_registry, input, response, command_query_request_id)?;
  (response.status == ChatCommandQueryStatus::Ok && !response.results.is_empty()).then_some(response.results.as_slice())
}

#[derive(Clone)]
struct CommandQueryResultRowProps {
  result: ChatCommandQueryResult,
  message_input: Signal<String>,
  command_registry: ChatCommandRegistry,
  selected_index: Signal<usize>,
  index: usize,
  selected: bool,
}

impl PartialEq for CommandQueryResultRowProps {
  fn eq(&self, other: &Self) -> bool {
    self.result == other.result && self.index == other.index && self.selected == other.selected
  }
}

impl DevtoolsInspectable for CommandQueryResultRowProps {}

struct CommandQueryResultRow;

impl Component for CommandQueryResultRow {
  type Props = CommandQueryResultRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    command_query_result_row(ctx, props)
  }
}

fn command_query_result_row(ctx: &mut Ctx, props: CommandQueryResultRowProps) -> Element {
  let background = if props.selected {
    BackgroundColor::Color(Color::from_hex("#232830"))
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  };
  let subtitle = command_query_result_subtitle(&props.result);
  let result = props.result.clone();
  let registry = props.command_registry.clone();
  let input = props.message_input.clone();

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(CHAT_COMMAND_SUGGESTION_ROW_HEIGHT)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(10.0)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(background)
    .transition(Transition::background_color().duration_ms(120))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#2B313A"))))
    .on_mouse_enter(move || props.selected_index.set(props.index))
    .on_click(move |_| fill_chat_command_query_result(&registry, &input, &result))
    .child(command_icon(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .clip()
        .spacing(theme::SpacingSize::Xs)
        .child(
          Text::new(&props.result.title)
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextPrimary)
            .nowrap(),
        )
        .child(
          Text::new(&subtitle)
            .variant(theme::TypographyStyle::Caption)
            .color(theme::PaletteColor::TextSecondary)
            .nowrap(),
        ),
    )
    .into()
}

fn command_query_result_subtitle(result: &ChatCommandQueryResult) -> String {
  if !result.subtitle.trim().is_empty() {
    return result.subtitle.clone();
  }
  if !result.kind.trim().is_empty() {
    return result.kind.clone();
  }
  result.value.clone()
}

fn fill_chat_command_query_result(
  command_registry: &ChatCommandRegistry,
  message_input: &Signal<String>,
  result: &ChatCommandQueryResult,
) {
  let input = message_input.get_untracked();
  if command_registry.live_query_for_input(&input).is_none() {
    return;
  }
  let value = if result.value.trim().is_empty() {
    result.title.trim()
  } else {
    result.value.trim()
  };
  if value.is_empty() {
    return;
  }
  let trimmed = input.trim_start();
  let leading_len = input.len().saturating_sub(trimmed.len());
  let Some(command_end) = trimmed.find(char::is_whitespace) else {
    return;
  };
  message_input.set(format!(
    "{}{} {}",
    &input[..leading_len],
    &trimmed[..command_end],
    value
  ));
}

fn command_suggestion_list_height(command_count: usize) -> f32 {
  let visible_rows = command_count.min(7) as f32;
  (visible_rows * CHAT_COMMAND_SUGGESTION_ROW_HEIGHT + 6.0).clamp(
    CHAT_COMMAND_SUGGESTION_ROW_HEIGHT + 6.0,
    CHAT_COMMAND_SUGGESTION_ROW_HEIGHT * 7.0 + 6.0,
  )
}

fn ensure_command_selection_visible(scroll_state: &ScrollState, active_index: usize, fallback_viewport_height: f32) {
  let viewport_height = scroll_state.viewport_height().max(fallback_viewport_height);
  let current_scroll = scroll_state.scroll_y();
  let row_top = active_index as f32 * CHAT_COMMAND_SUGGESTION_ROW_HEIGHT;
  let row_bottom = row_top + CHAT_COMMAND_SUGGESTION_ROW_HEIGHT;
  let viewport_bottom = current_scroll + viewport_height;
  let next_scroll = if row_top < current_scroll {
    row_top
  } else if row_bottom > viewport_bottom {
    row_bottom - viewport_height
  } else {
    return;
  };

  scroll_state.set_scroll_pending(scroll_state.scroll_x(), next_scroll);
}

#[derive(Clone)]
struct CommandSuggestionRowProps {
  fill: String,
  description: String,
  usage_parts: Vec<CommandUsagePart>,
  message_input: Signal<String>,
  selected_index: Signal<usize>,
  index: usize,
  selected: bool,
  invalid_feedback_phase: u8,
}

impl PartialEq for CommandSuggestionRowProps {
  fn eq(&self, other: &Self) -> bool {
    self.fill == other.fill
      && self.description == other.description
      && self.usage_parts == other.usage_parts
      && self.index == other.index
      && self.selected == other.selected
      && self.invalid_feedback_phase == other.invalid_feedback_phase
  }
}

impl DevtoolsInspectable for CommandSuggestionRowProps {}

struct CommandSuggestionRow;

impl Component for CommandSuggestionRow {
  type Props = CommandSuggestionRowProps;

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let props = ctx.props::<Self::Props>().clone();
    command_suggestion_row(ctx, props)
  }
}

fn command_suggestion_row(ctx: &mut Ctx, props: CommandSuggestionRowProps) -> Element {
  let fill = props.fill.clone();
  let background = if props.selected {
    BackgroundColor::Color(Color::from_hex("#232830"))
  } else {
    BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised)
  };

  Row::new()
    .width(Dimension::Pct(100.0))
    .height(CHAT_COMMAND_SUGGESTION_ROW_HEIGHT)
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Lg)
    .padding_vertical(10.0)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(background)
    .transition(Transition::background_color().duration_ms(120))
    .cursor(CursorIcon::Pointer)
    .hovered_style(Style::new().background(BackgroundColor::Color(Color::from_hex("#2B313A"))))
    .on_mouse_enter(move || props.selected_index.set(props.index))
    .on_click(move |_| props.message_input.set(fill.clone()))
    .child(command_icon(ctx))
    .child(
      Column::new()
        .width(Dimension::Pct(100.0))
        .flex(1.0)
        .spacing(theme::SpacingSize::Xs)
        .child(command_usage_row(props.usage_parts, props.invalid_feedback_phase))
        .child(
          Text::new(&props.description)
            .variant(theme::TypographyStyle::Link)
            .color(theme::PaletteColor::TextSecondary),
        ),
    )
    .into()
}

fn command_description(ctx: &mut Ctx, command: &CommandDefinition) -> String {
  if command.description_is_i18n_key {
    ctx.t(&command.description_key).to_string()
  } else {
    command.description_key.to_string()
  }
}

fn command_icon(ctx: &mut Ctx) -> Element {
  Row::new()
    .width(28.0)
    .height(28.0)
    .align_items(Alignment::Center)
    .justify(Justify::Center)
    .rounded(14.0)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfacePanel))
    .border_inside(1.0, theme::PaletteColor::BorderStrong)
    .child(ctx.mount::<LucideIcon>(LucideIconProps {
      icon: "terminal",
      size: 14.0,
      color: theme::palette().text_secondary,
    }))
    .into()
}

fn command_suggestion_title(title: &str) -> Element {
  Row::new()
    .width(Dimension::Pct(100.0))
    .height(CHAT_COMMAND_SUGGESTION_TITLE_HEIGHT)
    .align_items(Alignment::Center)
    .padding_horizontal(theme::SpacingSize::Lg)
    .background(BackgroundColor::Palette(theme::PaletteColor::SurfaceRaised))
    .child(
      Text::new(title)
        .variant(theme::TypographyStyle::Caption)
        .color(theme::PaletteColor::TextMuted),
    )
    .into()
}

fn command_usage_row(parts: Vec<CommandUsagePart>, invalid_feedback_phase: u8) -> Element {
  let mut row = Row::new()
    .width(Dimension::Pct(100.0))
    .align_items(Alignment::Center)
    .spacing(theme::SpacingSize::Sm)
    .wrap();

  for part in parts {
    let child: Element = match part {
      CommandUsagePart::Name(name) => Text::new(name.as_ref())
        .variant(theme::TypographyStyle::Heading)
        .color(theme::PaletteColor::TextPrimary)
        .into(),
      CommandUsagePart::Argument { label, invalid } => {
        command_argument_pill(label.as_ref(), invalid, invalid_feedback_phase)
      }
    };
    row = row.child(child);
  }

  row.into()
}

#[derive(Clone, PartialEq, Eq)]
enum CommandUsagePart {
  Name(Arc<str>),
  Argument { label: Arc<str>, invalid: bool },
}

fn command_usage_parts(usage: &str, input: &str, validate_missing: bool) -> Vec<CommandUsagePart> {
  let input_args = command_preview_input_args(input);
  let mut argument_index = 0usize;
  usage
    .split_whitespace()
    .map(|part| {
      if let Some(argument) = part.strip_prefix('{').and_then(|part| part.strip_suffix('}')) {
        let invalid = match input_args.get(argument_index) {
          Some(value) => !command_argument_value_valid(argument, value),
          None => validate_missing && command_argument_required(argument),
        };
        argument_index += 1;
        CommandUsagePart::Argument {
          label: command_argument_display_label(argument),
          invalid,
        }
      } else {
        CommandUsagePart::Name(Arc::from(part))
      }
    })
    .collect()
}

fn command_argument_display_label(argument: &str) -> Arc<str> {
  let Some((name, ty)) = command_argument_parts(argument) else {
    return Arc::from(argument);
  };
  Arc::from(format!("{name}:{}", command_argument_type_label(ty)))
}

fn command_argument_required(argument: &str) -> bool {
  let Some((name, ty)) = argument.split_once(':') else {
    return false;
  };
  !name.ends_with('?') && !ty.starts_with('?')
}

fn command_argument_pill(argument: &str, invalid: bool, invalid_feedback_phase: u8) -> Element {
  let (background, border, text_color) = if invalid {
    (
      BackgroundColor::Palette(theme::PaletteColor::DangerMuted),
      BackgroundColor::Color(theme::palette().danger.with_opacity(0.55)),
      theme::PaletteColor::Danger,
    )
  } else {
    (
      BackgroundColor::Color(Color::from_hex("#0B0C0E")),
      BackgroundColor::Palette(theme::PaletteColor::BorderStrong),
      theme::PaletteColor::TextSecondary,
    )
  };
  let shake_x = if invalid {
    command_invalid_shake_offset(invalid_feedback_phase)
  } else {
    0.0
  };

  Row::new()
    .height(22.0)
    .align_items(Alignment::Center)
    .padding_horizontal(6.0)
    .rounded(theme::RadiusSize::Md)
    .background(background)
    .border_inside(1.0, border)
    .transform(Transform2D::translate(shake_x, 0.0))
    .transition(Transition::transform().duration_ms(CHAT_COMMAND_INVALID_SHAKE_TRANSITION_MS))
    .child(
      Text::new(argument)
        .variant(theme::TypographyStyle::Button)
        .color(text_color),
    )
    .into()
}

fn command_invalid_shake_offset(phase: u8) -> f32 {
  match phase {
    1 => -8.0,
    2 => 7.0,
    3 => -5.0,
    4 => 4.0,
    5 => -2.0,
    _ => 0.0,
  }
}

fn command_preview_input_args(input: &str) -> Vec<String> {
  input
    .trim_start()
    .split_whitespace()
    .skip(1)
    .map(str::to_owned)
    .collect()
}

fn command_argument_value_valid(argument: &str, value: &str) -> bool {
  let Some((name, ty)) = command_argument_parts(argument) else {
    return true;
  };
  match ty {
    "u8" => value.parse::<u8>().is_ok_and(|value| name != "volume" || value <= 100),
    "u16" => value.parse::<u16>().is_ok(),
    "u32" => value.parse::<u32>().is_ok_and(|value| name != "userId" || value > 0),
    "u64" => value.parse::<u64>().is_ok(),
    "role" => matches!(
      value.to_ascii_lowercase().as_str(),
      "owner" | "admin" | "moderator" | "mod" | "user"
    ),
    "choice" | "string" => !value.trim().is_empty(),
    _ => true,
  }
}

fn command_argument_parts(argument: &str) -> Option<(&str, &str)> {
  let (name, ty) = argument.split_once(':')?;
  let ty = ty.strip_prefix('?').unwrap_or(ty);
  let name = name.strip_suffix('?').unwrap_or(name);
  Some((name, ty))
}

fn command_argument_type_label(ty: &str) -> &'static str {
  match ty {
    "u8" | "u16" | "u32" | "u64" => "Number",
    "string" => "String",
    "role" => "Role",
    "choice" => "Choice",
    _ => "Value",
  }
}

fn command_input_has_invalid_argument(
  command_registry: &ChatCommandRegistry,
  input: &str,
  _selected_index: &Signal<usize>,
) -> bool {
  let Some(command) = exact_chat_command_definition(command_registry, input) else {
    return false;
  };

  command_usage_parts(&command.usage, input, true)
    .into_iter()
    .any(|part| matches!(part, CommandUsagePart::Argument { invalid: true, .. }))
}

fn command_fill_text(command_name: &str) -> String {
  format!("{command_name} ")
}

fn submit_chat_if_valid(
  channel_id: Option<ChannelId>,
  message_input: &Signal<String>,
  command_selected_index: &Signal<usize>,
  command_invalid_feedback: &ChatCommandInvalidFeedback,
  command_registry: &ChatCommandRegistry,
  send_chat: &SendChatAction,
) {
  let text = message_input.get_untracked();
  let text = text.trim();
  if text.is_empty() {
    return;
  }

  if command_registry.has_commands()
    && command_input_has_invalid_argument(command_registry, &text, command_selected_index)
  {
    command_invalid_feedback.trigger();
    return;
  }

  run_chat_submission(channel_id, text, command_registry.clone(), send_chat);
  message_input.set(String::new());
}

fn run_chat_submission(
  channel_id: Option<ChannelId>,
  text: &str,
  command_registry: ChatCommandRegistry,
  send_chat: &SendChatAction,
) {
  send_chat.run(SendChatInput {
    channel_id,
    text: text.to_owned(),
    command_registry,
  });
}

#[cfg(test)]
#[path = "../../../../tests/unit/ui/lobby/chat/composer.rs"]
mod tests;
