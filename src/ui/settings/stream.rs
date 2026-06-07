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

pub struct SettingsStreamScreen;

impl Component for SettingsStreamScreen {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let notice_title = ctx.t("settings.not_wired.title");
    let notice_description = ctx.t("settings.stream.not_wired");
    let notice = muted_notice(ctx, &notice_title, &notice_description);
    let resolution = setting_row(
      &ctx.t("settings.stream.resolution"),
      &ctx.t("settings.stream.resolution.description"),
      disabled_select("1920 x 1080"),
      false,
    );
    let codec = setting_row(
      &ctx.t("settings.stream.codec"),
      &ctx.t("settings.stream.codec.description"),
      disabled_select("AV1"),
      false,
    );
    let frame_rate = setting_row(
      &ctx.t("settings.stream.frame_rate"),
      &ctx.t("settings.stream.frame_rate.description"),
      disabled_select("60 fps"),
      false,
    );
    let content = page_stack()
      .child(header(
        &ctx.t("settings.stream.title"),
        &ctx.t("settings.stream.description"),
      ))
      .child(notice)
      .child(
        Column::new()
          .width(Dimension::Pct(100.0))
          .spacing(theme::SpacingSize::Md)
          .opacity(0.6)
          .child(section_label(&ctx.t("settings.stream.section.capture"), false))
          .child(resolution)
          .child(codec)
          .child(frame_rate),
      );

    screen(ctx, SettingsPage::Stream, content)
  }
}
