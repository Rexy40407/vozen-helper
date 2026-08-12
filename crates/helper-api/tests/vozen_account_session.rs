use helper_api::is_trusted_vozen_account_identity;

#[test]
fn trusted_vozen_account_identity_requires_the_expected_application_and_guild_scope() {
    assert!(is_trusted_vozen_account_identity(
        "1523826014935842997",
        "1523826014935842997",
        &["identify".into(), "email".into(), "guilds".into()],
    ));
    assert!(!is_trusted_vozen_account_identity(
        "1523826014935842997",
        "another-application",
        &["identify".into(), "guilds".into()],
    ));
    assert!(!is_trusted_vozen_account_identity(
        "1523826014935842997",
        "1523826014935842997",
        &["identify".into(), "email".into()],
    ));
}
