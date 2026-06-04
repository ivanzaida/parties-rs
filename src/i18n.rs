use lurq::app::i18n::I18n;

pub fn setup(i18n: &I18n) {
  i18n.set_locale("en");
  i18n.set_fallback_locale("en");
  i18n.add_resources(
    "en",
    "translation",
    [
      ("identity.action.back", "Back"),
      ("identity.action.continue_saved", "I saved it - continue"),
      ("identity.action.import_key", "Import key"),
      ("identity.action.restore", "Restore identity"),
      ("identity.import.caption", "IMPORT"),
      (
        "identity.import.desc",
        "Use this only when you need to migrate a raw identity key manually.",
      ),
      ("identity.import.field_label", "HEX PRIVATE KEY"),
      ("identity.import.heading", "Private key"),
      (
        "identity.import.meta_desc",
        "Prefer seed restore unless you explicitly exported a private key.",
      ),
      ("identity.import.meta_title", "Advanced path"),
      ("identity.import.placeholder", "a3f1b2c4d5e691cc..."),
      ("identity.import.status.accepted", "Private key format accepted."),
      (
        "identity.import.status.empty",
        "Never paste keys from chat or unknown files.",
      ),
      (
        "identity.import.status.invalid",
        "Invalid private key. Must be 64 hex characters.",
      ),
      ("identity.import.status.save_failed", "Failed to save identity."),
      ("identity.import.title", "Import private key"),
      ("identity.restore.caption", "RESTORE"),
      (
        "identity.restore.desc",
        "Paste the 12-word seed phrase for an existing Parties identity.",
      ),
      ("identity.restore.field_label", "SEED PHRASE"),
      ("identity.restore.heading", "Seed phrase"),
      (
        "identity.restore.meta_desc",
        "The phrase is checked locally before any network call.",
      ),
      ("identity.restore.meta_title", "Offline validation"),
      (
        "identity.restore.placeholder",
        "abandon ability able about above absent ...",
      ),
      ("identity.restore.status.accepted", "Seed phrase format accepted."),
      (
        "identity.restore.status.empty",
        "12 words required. Extra spaces are ignored.",
      ),
      (
        "identity.restore.status.invalid",
        "Invalid seed phrase. Enter 12 known words.",
      ),
      ("identity.restore.status.save_failed", "Failed to save identity."),
      ("identity.restore.title", "Restore identity"),
      ("identity.seed.caption", "BACKUP"),
      (
        "identity.seed.desc",
        "This phrase is the only recovery path for your identity. Store it before joining servers.",
      ),
      ("identity.seed.heading", "Recovery phrase"),
      ("identity.seed.meta_desc", "The app cannot recover a lost seed phrase."),
      ("identity.seed.meta_title", "Backup required"),
      (
        "identity.seed.save_failed_desc",
        "SQLite storage is unavailable. Try again before continuing.",
      ),
      ("identity.seed.save_failed_title", "Identity not saved"),
      ("identity.seed.title", "Save recovery phrase"),
      ("identity.setup.caption", "IDENTITY"),
      (
        "identity.setup.desc",
        "Parties uses a local cryptographic identity for names, server trust, and peer verification.",
      ),
      ("identity.setup.heading", "Choose setup method"),
      (
        "identity.setup.meta_desc",
        "Create one now or restore an existing key from backup.",
      ),
      ("identity.setup.meta_title", "No local identity found"),
      (
        "identity.setup.option.generate_desc",
        "Creates a seed phrase and a new peer fingerprint.",
      ),
      ("identity.setup.option.generate_title", "Generate new identity"),
      (
        "identity.setup.option.import_desc",
        "Paste a raw 64-character private key.",
      ),
      ("identity.setup.option.import_title", "Import private key"),
      (
        "identity.setup.option.restore_desc",
        "Use a saved 12-word backup from another install.",
      ),
      ("identity.setup.option.restore_title", "Restore seed phrase"),
      ("identity.setup.storage_note", "Seed material stays on this device."),
      (
        "identity.setup.storage_unavailable",
        "Local storage is unavailable. Identity cannot be saved.",
      ),
      ("identity.setup.title", "Create identity"),
      ("loading.caption", "LOADING"),
      ("loading.card_title", "Preparing Parties"),
      (
        "loading.desc",
        "Checking local identity and saved servers before opening Parties.",
      ),
      ("loading.identity.desc", "Opening local key storage"),
      ("loading.identity.title", "Identity"),
      (
        "loading.meta_desc",
        "Identity storage and server list load asynchronously.",
      ),
      ("loading.meta_title", "Startup checks running"),
      ("loading.servers.desc", "Reading saved endpoints"),
      ("loading.servers.title", "Servers"),
      ("loading.status.desc", "This usually finishes in a moment."),
      ("loading.status.title", "Loading identity and servers"),
      ("loading.title", "Loading workspace"),
      ("server_connect.action.connect", "Connect"),
      ("server_connect.address.label", "SERVER ADDRESS"),
      ("server_connect.address.placeholder", "127.0.0.1:7800"),
      ("server_connect.caption", "CONNECT"),
      (
        "server_connect.desc",
        "Enter an endpoint, choose your display name, and review trust before joining.",
      ),
      ("server_connect.display_name.label", "DISPLAY NAME"),
      ("server_connect.display_name.placeholder", "alice"),
      ("server_connect.heading", "Server details"),
      ("server_connect.invite_seed.label", "INVITE SEED"),
      ("server_connect.invite_seed.placeholder", "optional server password"),
      (
        "server_connect.meta_desc",
        "New server fingerprints are saved after confirmation.",
      ),
      ("server_connect.meta_title", "Trust on first use"),
      ("server_connect.title", "Connect to server"),
      (
        "server_connect.trust.empty",
        "fingerprint will be shown before media starts",
      ),
      ("server_connect.trust.label", "TRUST PREVIEW"),
      (
        "server_connect.trust.pending",
        "fingerprint will be checked before media starts",
      ),
      ("server_select.action.add", "Add"),
      ("server_select.action.drop_identity", "Drop identity"),
      ("server_select.caption", "SERVERS"),
      (
        "server_select.desc",
        "Join a saved server or add a new endpoint. Trust fingerprints are shown before connecting.",
      ),
      (
        "server_select.drop_identity_failed",
        "Failed to drop identity from local storage.",
      ),
      ("server_select.heading", "Saved servers"),
      ("server_select.meta_desc", "Localhost has no saved fingerprint yet."),
      ("server_select.meta_title", "3 trusted servers"),
      ("server_select.title", "Choose server"),
    ],
  );
}
