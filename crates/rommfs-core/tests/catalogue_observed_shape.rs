//! Portable measurement only; this cannot predict Windows mount latency.
use rommfs_core::catalog::build_catalogue;
use rommfs_core::romm::RommClient;
use rommfs_fixture::{contract, FixtureServer, ResponseSpec};
use std::time::Instant;

#[test]
fn observed_library_shape_fetches_each_page_once_without_rom_bodies() {
    let server = FixtureServer::start();
    let slugs: Vec<_> = (0..25).map(|p| format!("platform{p:02}")).collect();
    let platform_rows: Vec<_> = slugs
        .iter()
        .enumerate()
        .map(|(p, slug)| (p as i64 + 1, slug.as_str(), slug.as_str(), slug.as_str()))
        .collect();
    server.on(
        "GET",
        "/api/platforms",
        ResponseSpec::Json {
            status: 200,
            body: contract::platforms(&platform_rows),
        },
    );
    let roms: Vec<_> = (0..2448)
        .map(|r| {
            contract::rom(
                r + 1,
                &slugs[r as usize % 25],
                &format!("Game {r:04}.rom"),
                4096,
                "version",
            )
        })
        .collect();
    let pages = roms
        .chunks(200)
        .enumerate()
        .map(|(page, items)| ResponseSpec::Json {
            status: 200,
            body: contract::roms_page(items, roms.len(), 200, page * 200),
        })
        .collect();
    server.on_sequence("GET", "/api/roms?", pages);
    let client = RommClient::new(server.url()).unwrap();
    let started = Instant::now();
    let platforms = client.platforms().unwrap();
    let fetched = client.all_roms().unwrap();
    let fetch_time = started.elapsed();
    let projection_started = Instant::now();
    let catalogue = build_catalogue(server.url(), &platforms, &fetched, |warning| {
        panic!("{warning}")
    })
    .unwrap();
    eprintln!("observed-shape fixture: platforms=25 ROMs=2448 pages=13 HTTP={fetch_time:?} projection={:?}",projection_started.elapsed());
    assert_eq!(catalogue.platforms.len(), 25);
    assert_eq!(catalogue.entries.len(), 2448);
    assert_eq!(server.count_requests("GET", "/api/platforms"), 1);
    assert_eq!(server.count_requests("GET", "/api/roms?"), 13);
    assert_eq!(server.count_requests("GET", "/api/roms/"), 0);
    let requests = server.requests();
    for page in 0..13 {
        let offset = format!("offset={}", page * 200);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.target.split('&').any(|part| part == offset))
                .count(),
            1
        );
    }
}
