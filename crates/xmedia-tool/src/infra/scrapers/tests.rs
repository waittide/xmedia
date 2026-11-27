use crate::domain::config::ProviderCredentials;
use crate::infra::scrapers::build_default_scraper_providers;

#[test]
fn default_registry_exposes_twenty_eight_providers_and_checks_credentials_and_adult_gate() {
    let providers = build_default_scraper_providers();
    assert_eq!(providers.len(), 28);

    let empty_creds = ProviderCredentials::default();
    let configured_ids: Vec<String> = providers
        .iter()
        .filter(|p| p.is_configured(&empty_creds))
        .map(|p| p.descriptor().id)
        .collect();
    assert_eq!(
        configured_ids,
        vec![
            "douban",
            "neodb",
            "xinhua",
            "openlibrary",
            "qidian",
            "fanqie",
            "qimao",
            "17k",
            "ciweimao",
            "sfacg",
            "faloo",
            "jjwxc",
            "qq",
            "netease",
            "kugou",
            "kuwo",
            "migu",
            "taihe",
            "qishui",
            "itunes",
            "musicbrainz",
            "lrclib",
            "bangumi",
            "anilist",
            "mangadex",
        ]
    );

    let with_tmdb = ProviderCredentials {
        tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
        ..Default::default()
    };
    let without_adult_ids: Vec<String> = providers
        .iter()
        .filter(|p| !p.is_configured(&with_tmdb))
        .map(|p| p.descriptor().id)
        .collect();
    assert_eq!(without_adult_ids, vec!["nhentai", "hanime1"]);

    let with_all = ProviderCredentials {
        tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
        enable_adult_scrapers: true,
        ..Default::default()
    };
    assert!(providers.iter().all(|p| p.is_configured(&with_all)));
}
