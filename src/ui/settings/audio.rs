use lurq::{
  app::{component::Component, ctx::Ctx},
  components::Column,
  node::{Element, dimension::Dimension},
};

use crate::{
  theme,
  ui::settings::shell::{
    SettingsPage, disabled_select, header, muted_notice, page_stack, screen, section_label, setting_row,
  },
};

pub struct SettingsAudioScreen;

impl Component for SettingsAudioScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let notice_title = ctx.t("settings.not_wired.title");
    let notice_description = ctx.t("settings.audio.not_wired");
    let notice = muted_notice(ctx, &notice_title, &notice_description);
    let input_device = setting_row(
      &ctx.t("settings.audio.input.device"),
      &ctx.t("settings.audio.input.device.description"),
      disabled_select(&ctx.t("settings.audio.device.system")),
      false,
    );
    let input_gain = setting_row(
      &ctx.t("settings.audio.input.gain"),
      &ctx.t("settings.audio.input.gain.description"),
      disabled_select("100%"),
      false,
    );
    let output_device = setting_row(
      &ctx.t("settings.audio.output.device"),
      &ctx.t("settings.audio.output.device.description"),
      disabled_select(&ctx.t("settings.audio.device.system")),
      false,
    );
    let output_volume = setting_row(
      &ctx.t("settings.audio.output.volume"),
      &ctx.t("settings.audio.output.volume.description"),
      disabled_select("100%"),
      false,
    );
    let content = page_stack()
      .child(header(
        &ctx.t("settings.audio.title"),
        &ctx.t("settings.audio.description"),
      ))
      .child(notice)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Md)
          .opacity(0.6)
          .child(section_label(&ctx.t("settings.audio.input"), false))
          .child(input_device)
          .child(input_gain)
          .child(section_label(&ctx.t("settings.audio.output"), false))
          .child(output_device)
          .child(output_volume),
      );

    screen(ctx, SettingsPage::Audio, content)
  }
}
