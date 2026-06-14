mod audio;
mod identity;
mod notifications;
mod overview;
mod refresh_button;
mod saved_servers;
mod shell;
mod stream;
mod toggle;

pub use audio::SettingsAudioScreen;
pub use identity::SettingsIdentityScreen;
use lurq::{
  app::{component::Component, ctx::Ctx},
  node::Element,
};
pub use notifications::SettingsNotificationsScreen;
pub use overview::SettingsOverviewScreen;
pub use saved_servers::SettingsSavedServersScreen;
pub use shell::{SettingsPage, SettingsPopupHandle};
pub use stream::SettingsStreamScreen;

pub struct SettingsPopup;

impl Component for SettingsPopup {
  type Props = ();

  fn create(_ctx: &mut Ctx) -> Self {
    Self
  }

  fn render(&self, ctx: &mut Ctx) -> impl Into<Element> {
    let Some(handle) = ctx.use_context::<SettingsPopupHandle>() else {
      return ctx.mount::<SettingsOverviewScreen>(());
    };

    match handle.page() {
      SettingsPage::Overview => ctx.mount::<SettingsOverviewScreen>(()),
      SettingsPage::Identity => ctx.mount::<SettingsIdentityScreen>(()),
      SettingsPage::Servers => ctx.mount::<SettingsSavedServersScreen>(()),
      SettingsPage::Audio => ctx.mount::<SettingsAudioScreen>(()),
      SettingsPage::Notifications => ctx.mount::<SettingsNotificationsScreen>(()),
      SettingsPage::Stream => ctx.mount::<SettingsStreamScreen>(()),
    }
  }
}
