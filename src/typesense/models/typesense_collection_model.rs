use serde::Deserialize;

/// One entry of Typesense's `GET /collections` response. Only the fields the
/// exporter uses are parsed; the schema and the rest are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct TypesenseCollection {
    pub name: String,
    #[serde(default)]
    pub num_documents: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_document_count_ignoring_the_schema() {
        let collections: Vec<TypesenseCollection> = serde_json::from_str(
            r#"[{"name": "products", "num_documents": 18, "created_at": 1,
                 "fields": [{"name": "title", "type": "string"}]},
                {"name": "empty"}]"#,
        )
        .unwrap();

        assert_eq!(collections[0].name, "products");
        assert_eq!(collections[0].num_documents, Some(18.0));
        assert_eq!(collections[1].num_documents, None);
    }
}
