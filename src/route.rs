use lurq::DevtoolsInspectable;

#[derive(Clone, PartialEq, DevtoolsInspectable)]
pub enum Route {
    IdentityGenerate,
    SeedPhraseDisplay,
    IdentityRestore,
    IdentityImportKey,
    Connect,
    TofuWarning,
    Lobby,
    LobbyScreenShare,
    Servers,
    Settings,
    SettingsIdentity,
    SettingsAppearance,
    SettingsAbout,
}
