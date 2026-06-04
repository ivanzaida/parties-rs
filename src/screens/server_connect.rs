use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, Text, TextInput},
  core::Signal,
  layout::text_style::{FontWeight, TextStyle},
  node::{BackgroundColor, Element, color::Color, dimension::Dimension},
};

use crate::{
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, STEP_CHOOSE_SERVER, action_button, text_style},
  theme,
};

fn meta_card(title: &str, body: &str) -> Column {
  Column::new()
    .width(INTRO_WIDTH)
    .spacing(8.0)
    .padding(12.0)
    .rounded(6.0)
    .background(BackgroundColor::Palette(theme::BG_TERTIARY))
    .border_inside(1.0, Color::from_hex(BORDER))
    .child(shared::dot(BackgroundColor::Palette(theme::ORANGE)))
    .child(Text::new(title).variant(theme::TYP_BUTTON))
    .child(Text::new(body).variant(theme::TYP_LINK))
}

fn text_field(value: Signal<String>, label: &str, placeholder: &str) -> Column {
  let value_style = text_style("JetBrains Mono", 12.0, FontWeight::Medium, "#F4F4F2", 1.2);
  let placeholder_style = TextStyle {
    color: theme::TEXT_MUTED_COLOR,
    ..value_style.clone()
  };

  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(7.0)
    .child(shared::styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      theme::TEXT_MUTED_COLOR,
      1.2,
    ))
    .child(
      TextInput::styled(value, value_style)
        .width(Dimension::Pct(100.0))
        .height(38.0)
        .padding_horizontal(10.0)
        .rounded(5.0)
        .background(BackgroundColor::Palette(theme::BG_SECONDARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .caret_color(theme::ACCENT_COLOR)
        .placeholder(placeholder)
        .placeholder_style(placeholder_style)
        .single_line(),
    )
}

fn trust_preview(label: &str, value: &str) -> Column {
  Column::new()
    .width(Dimension::Pct(100.0))
    .spacing(6.0)
    .padding(10.0)
    .rounded(5.0)
    .background(BackgroundColor::Palette(theme::BG_ELEVATED))
    .border_inside(1.0, Color::from_hex(BORDER))
    .child(shared::styled_text(
      label,
      "JetBrains Mono",
      10.0,
      FontWeight::Bold,
      theme::TEXT_MUTED_COLOR,
      1.2,
    ))
    .child(shared::styled_text(
      value,
      "JetBrains Mono",
      10.0,
      FontWeight::Medium,
      theme::TEXT_MUTED_COLOR,
      1.2,
    ))
}

pub struct ServerConnect {
  address: Signal<String>,
  display_name: Signal<String>,
  invite_seed: Signal<String>,
}

impl Component for ServerConnect {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      address: ctx.signal(String::new()),
      display_name: ctx.signal(String::new()),
      invite_seed: ctx.signal(String::new()),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let step = ctx.use_context::<Signal<u8>>();
    let address = self.address.get();
    let trust_text = if address.trim().is_empty() {
      ctx.t("server_connect.trust.empty")
    } else {
      ctx.t("server_connect.trust.pending")
    };

    shared::identity_screen(
      Column::new()
        .width(INTRO_WIDTH)
        .spacing(18.0)
        .child(Text::new(&ctx.t("server_connect.caption")).variant(theme::TYP_CAPTION))
        .child(Text::new(&ctx.t("server_connect.title")).variant(theme::TYP_TITLE))
        .child(Text::new(&ctx.t("server_connect.desc")).variant(theme::TYP_DESC))
        .child(meta_card(
          &ctx.t("server_connect.meta_title"),
          &ctx.t("server_connect.meta_desc"),
        )),
      Column::new()
        .width(CARD_WIDTH)
        .spacing(14.0)
        .padding(18.0)
        .rounded(8.0)
        .background(BackgroundColor::Palette(theme::BG_TERTIARY))
        .border_inside(1.0, Color::from_hex(BORDER))
        .child(Text::new(&ctx.t("server_connect.heading")).variant(theme::TYP_HEADING))
        .child(text_field(
          self.address.clone(),
          &ctx.t("server_connect.address.label"),
          &ctx.t("server_connect.address.placeholder"),
        ))
        .child(text_field(
          self.display_name.clone(),
          &ctx.t("server_connect.display_name.label"),
          &ctx.t("server_connect.display_name.placeholder"),
        ))
        .child(text_field(
          self.invite_seed.clone(),
          &ctx.t("server_connect.invite_seed.label"),
          &ctx.t("server_connect.invite_seed.placeholder"),
        ))
        .child(trust_preview(&ctx.t("server_connect.trust.label"), &trust_text))
        .child(action_button(&ctx.t("server_connect.action.connect"), true))
        .child({
          let button = action_button(&ctx.t("identity.action.back"), false);
          if let Some(step) = step {
            button.on_click(move |_| step.set(STEP_CHOOSE_SERVER))
          } else {
            button
          }
        }),
    )
  }
}
