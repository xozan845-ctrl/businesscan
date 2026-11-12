use nicascan::db::business::{get_full, import_batch, search};
use nicascan::db::Database;
use nicascan::model::{BusinessRecord, Contact, ContactKind, SearchQuery, SortBy};
use nicascan::normalize::normalize_phone;

fn record(source: &str, external_id: &str, name: &str) -> BusinessRecord {
    let mut record = BusinessRecord::new(source, name);
    record.external_id = Some(external_id.to_string());
    record.primary_category = Some("Restaurante".to_string());
    record.business_type = Some("amenity=restaurant".to_string());
    record.address.municipio = Some("León".to_string());
    record.address.departamento = Some("León".to_string());
    record.address.barrio = Some("Barrio San Juan".to_string());
    record.latitude = Some(12.435);
    record.longitude = Some(-86.878);
    record.confidence = 0.7;
    record
}

#[test]
fn fts5_is_available_in_bundled_sqlite() {
    let db = Database::open_memory().unwrap();
    assert!(db.fts_enabled());
}

#[test]
fn insert_and_search_by_accent_insensitive_text() {
    let mut db = Database::open_memory().unwrap();
    let mut rec = record("osm", "node/1", "Restaurante El Sabor NICARAGÜENSE");
    rec.contacts.push(
        Contact::new(ContactKind::Phone, "8888-7777")
            .with_source("osm")
            .primary(),
    );
    rec.contacts
        .push(Contact::new(ContactKind::Website, "elsabor.ni").with_source("osm"));
    import_batch(&mut db, &[rec]).unwrap();

    let query = SearchQuery {
        text: Some("nicaraguense".into()),
        ..Default::default()
    };
    let results = search(&db, &query).unwrap();
    assert_eq!(
        results.len(),
        1,
        "la búsqueda debe ignorar acentos y tolerar prefijos"
    );
    assert_eq!(results[0].phone_primary.as_deref(), Some("+50588887777"));
    assert_eq!(results[0].website.as_deref(), Some("https://elsabor.ni"));
    assert!(results[0].address.barrio.is_some());
}

#[test]
fn filters_by_department_municipality_and_contacts() {
    let mut db = Database::open_memory().unwrap();
    let mut leon = record("osm", "node/1", "Farmacia León");
    leon.contacts.push(
        Contact::new(ContactKind::Phone, "2222-3333")
            .with_source("osm")
            .primary(),
    );
    let mut managua = record("osm", "node/2", "Farmacia Managua");
    managua.address.municipio = Some("Managua".into());
    managua.address.departamento = Some("Managua".into());
    managua.latitude = Some(12.136);
    managua.longitude = Some(-86.251);
    import_batch(&mut db, &[leon, managua]).unwrap();

    let query = SearchQuery {
        departamento: Some("león".into()),
        municipio: Some("león".into()),
        has_phone: true,
        ..Default::default()
    };
    let results = search(&db, &query).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Farmacia León");

    let geo = SearchQuery {
        lat: Some(12.136),
        lon: Some(-86.251),
        radius_m: Some(1_000.0),
        sort: SortBy::Distance,
        ..Default::default()
    };
    let results = search(&db, &geo).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "Farmacia Managua");
    assert!(results[0].distance_m.unwrap() < 10.0);
}

#[test]
fn merges_duplicates_by_phone_and_keeps_both_sources() {
    let mut db = Database::open_memory().unwrap();
    let mut osm = record("osm", "node/1", "Pizza Nostra");
    osm.contacts.push(
        Contact::new(ContactKind::Phone, "+505 2222-4444")
            .with_source("osm")
            .primary(),
    );
    osm.contacts
        .push(Contact::new(ContactKind::Facebook, "pizzanostra").with_source("osm"));

    let mut google = record("google", "ChIJ123", "Pizza Nostra");
    google
        .contacts
        .push(Contact::new(ContactKind::Phone, "22224444").with_source("google"));
    google
        .contacts
        .push(Contact::new(ContactKind::Website, "pizzanostra.ni").with_source("google"));
    google.rating = Some(4.7);
    google.review_count = Some(200);
    google.address.barrio = None;

    let results = import_batch(&mut db, &[osm, google]).unwrap();
    assert_eq!(results[0].action, nicascan::model::UpsertAction::Inserted);
    assert_eq!(results[1].action, nicascan::model::UpsertAction::Merged);
    assert_eq!(
        results[1].match_reason,
        Some(nicascan::model::MatchReason::Phone)
    );

    let all = search(&db, &SearchQuery::default()).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].source_count, 2);
    assert_eq!(all[0].rating, Some(4.7));
    assert_eq!(all[0].review_count, Some(200));
    assert_eq!(all[0].website.as_deref(), Some("https://pizzanostra.ni"));

    let full = get_full(&db, nicascan::normalize::short_id(&all[0].id), false).unwrap();
    assert_eq!(full.sources.len(), 2);
    assert_eq!(full.contacts.len(), 3);
    assert!(full
        .contacts
        .iter()
        .any(|c| c.kind == ContactKind::Facebook));
}

#[test]
fn merges_duplicates_by_name_and_proximity() {
    let mut db = Database::open_memory().unwrap();
    let a = record("osm", "node/1", "Hotel Colonial");
    let mut b = record("osm", "node/2", "Hotel Colonial");
    b.latitude = Some(12.4351);
    b.longitude = Some(-86.8781);
    import_batch(&mut db, &[a, b]).unwrap();
    let all = search(&db, &SearchQuery::default()).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].source_count, 1);
}

#[test]
fn normalizes_phones_for_storage() {
    assert_eq!(
        normalize_phone("(505) 8888-7777").as_deref(),
        Some("+50588887777")
    );
}
