use std::io::{Cursor, Read};

use ironcalc_base::{FrozenEvaluationContext, Model};

#[test]
fn frozen_export_uses_model_time_for_modified_metadata() -> Result<(), Box<dyn std::error::Error>> {
    for (milliseconds, expected) in [
        (1_679_319_865_208, "2023-03-20T13:44:25Z"),
        (-1, "1969-12-31T23:59:59Z"),
        (-1001, "1969-12-31T23:59:58Z"),
    ] {
        let context = FrozenEvaluationContext::new(milliseconds, [7; 32])?;
        let mut model = Model::new_empty_with_frozen_context("frozen", "en", "UTC", "en", context)?;
        model.set_user_input(0, 1, 1, "=NOW()+RAND()".to_owned())?;
        model.evaluate_frozen()?;
        let writer = super::super::save_xlsx_to_writer(&model, Cursor::new(Vec::new()))?;
        let mut archive = zip::ZipArchive::new(Cursor::new(writer.into_inner()))?;
        let mut xml = String::new();
        archive
            .by_name("docProps/core.xml")?
            .read_to_string(&mut xml)?;
        let document = roxmltree::Document::parse(&xml)?;
        let created = document
            .descendants()
            .find(|n| n.has_tag_name(("http://purl.org/dc/terms/", "created")))
            .and_then(|n| n.text());
        let modified = document
            .descendants()
            .find(|n| n.has_tag_name(("http://purl.org/dc/terms/", "modified")))
            .and_then(|n| n.text());
        assert_eq!(created, Some(expected));
        assert_eq!(modified, created);
    }
    Ok(())
}
