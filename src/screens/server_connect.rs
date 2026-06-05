use lurq::{
  app::{component::Component, ctx::Ctx},
  components::{Column, FormHandle, FormOptions, FormProps, Text, validators},
  layout::text_style::FontWeight,
  node::{BackgroundColor, Element, color::Color, dimension::Dimension},
};

use crate::{
  screens::shared::{self, BORDER, CARD_WIDTH, INTRO_WIDTH, ROUTE_CHOOSE_SERVER, action_button},
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
  form: FormHandle,
}

impl Component for ServerConnect {
  type Props = ();

  fn create(ctx: &mut Ctx) -> Self {
    Self {
      form: ctx.form(
        FormOptions::new()
          .field("server_address", "")
          .field("display_name", "")
          .field("invite_seed", "")
          .validate_string("server_address", validators::required("Server address is required.")),
      ),
    }
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let navigator = ctx.navigator();
    let address = self.form.string("server_address");
    let trust_text = if address.get().trim().is_empty() {
      ctx.t("server_connect.trust.empty")
    } else {
      ctx.t("server_connect.trust.pending")
    };
    let server_address_error = self.form.error("server_address").get();
    let display_name_error = self.form.error("display_name").get();
    let invite_seed_error = self.form.error("invite_seed").get();
    eprintln!("server_connect render errors={:?}", self.form.errors());

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
      ctx.form_view_with(
        FormProps::new(self.form.clone().on_invalid(|errors| {
          eprintln!("server_connect invalid errors={errors:?}");
        })),
        |ctx| {
          Column::new()
            .width(CARD_WIDTH)
            .spacing(14.0)
            .padding(18.0)
            .rounded(8.0)
            .background(BackgroundColor::Palette(theme::BG_TERTIARY))
            .border_inside(1.0, Color::from_hex(BORDER))
            .child(Text::new(&ctx.t("server_connect.heading")).variant(theme::TYP_HEADING))
            .child(ctx.mount_keyed::<shared::FormTextInput>(
              if server_address_error.is_some() {
                "server_address-invalid"
              } else {
                "server_address-valid"
              },
              shared::FormTextInputProps::new(
                self.form.string_control("server_address"),
                ctx.t("server_connect.address.label"),
                ctx.t("server_connect.address.placeholder"),
                38.0,
              ),
            ))
            .child(ctx.mount_keyed::<shared::FormTextInput>(
              if display_name_error.is_some() {
                "display_name-invalid"
              } else {
                "display_name-valid"
              },
              shared::FormTextInputProps::new(
                self.form.string_control("display_name"),
                ctx.t("server_connect.display_name.label"),
                ctx.t("server_connect.display_name.placeholder"),
                38.0,
              ),
            ))
            .child(ctx.mount_keyed::<shared::FormTextInput>(
              if invite_seed_error.is_some() {
                "invite_seed-invalid"
              } else {
                "invite_seed-valid"
              },
              shared::FormTextInputProps::new(
                self.form.string_control("invite_seed"),
                ctx.t("server_connect.invite_seed.label"),
                ctx.t("server_connect.invite_seed.placeholder"),
                38.0,
              ),
            ))
            .child(trust_preview(&ctx.t("server_connect.trust.label"), &trust_text))
            .child(shared::submit_action_button(
              &ctx.t("server_connect.action.connect"),
              true,
            ))
            .child({
              let button = action_button(&ctx.t("identity.action.back"), false);
              if let Some(navigator) = navigator {
                button.on_click(move |_| navigator.push(ROUTE_CHOOSE_SERVER))
              } else {
                button
              }
            })
        },
      ),
    )
  }
}
