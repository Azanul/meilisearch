mod local_index_controller;
mod updates;

pub use local_index_controller::LocalIndexController;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use milli::Index;
use milli::update::{IndexDocumentsMethod, UpdateFormat, DocumentAdditionResult};
use serde::{Serialize, Deserialize, de::Deserializer};
use uuid::Uuid;

pub use updates::{Processed, Processing, Failed};

pub type UpdateStatus = updates::UpdateStatus<UpdateMeta, UpdateResult, String>;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IndexMetadata {
    pub name: String,
    uuid: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    primary_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum UpdateMeta {
    DocumentsAddition { method: IndexDocumentsMethod, format: UpdateFormat },
    ClearDocuments,
    DeleteDocuments,
    Settings(Settings),
    Facets(Facets),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Facets {
    pub level_group_size: Option<NonZeroUsize>,
    pub min_level_size: Option<NonZeroUsize>,
}

fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where T: Deserialize<'de>,
      D: Deserializer<'de>
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(
        default,
        deserialize_with = "deserialize_some",
        skip_serializing_if = "Option::is_none",
    )]
    pub displayed_attributes: Option<Option<Vec<String>>>,

    #[serde(
        default,
        deserialize_with = "deserialize_some",
        skip_serializing_if = "Option::is_none",
    )]
    pub searchable_attributes: Option<Option<Vec<String>>>,

    #[serde(default)]
    pub faceted_attributes: Option<Option<HashMap<String, String>>>,

    #[serde(
        default,
        deserialize_with = "deserialize_some",
        skip_serializing_if = "Option::is_none",
    )]
    pub criteria: Option<Option<Vec<String>>>,
}

impl Settings {
    pub fn cleared() -> Self {
        Self {
            displayed_attributes: Some(None),
            searchable_attributes: Some(None),
            faceted_attributes: Some(None),
            criteria: Some(None),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UpdateResult {
    DocumentsAddition(DocumentAdditionResult),
    DocumentDeletion { deleted: usize },
    Other,
}

#[derive(Clone, Debug)]
pub struct IndexSettings {
    pub name: Option<String>,
    pub primary_key: Option<String>,
}

/// The `IndexController` is in charge of the access to the underlying indices. It splits the logic
/// for read access which is provided thanks to an handle to the index, and write access which must
/// be provided. This allows the implementer to define the behaviour of write accesses to the
/// indices, and abstract the scheduling of the updates. The implementer must be able to provide an
/// instance of `IndexStore`
pub trait IndexController {

    /*
     * Write operations
     *
     * Logic for the write operation need to be provided by the implementer, since they can be made
     * asynchronous thanks to an update_store for example.
     *
     * */

    /// Perform document addition on the database. If the provided index does not exist, it will be
    /// created when the addition is applied to the index.
    fn add_documents<S: AsRef<str>>(
        &self,
        index: S,
        method: IndexDocumentsMethod,
        format: UpdateFormat,
        data: &[u8],
    ) -> anyhow::Result<UpdateStatus>;

    /// Clear all documents in the given index.
    fn clear_documents(&self, index: impl AsRef<str>) -> anyhow::Result<UpdateStatus>;

    /// Delete all documents in `document_ids`.
    fn delete_documents(&self, index: impl AsRef<str>, document_ids: Vec<String>) -> anyhow::Result<UpdateStatus>;

    /// Updates an index settings. If the index does not exist, it will be created when the update
    /// is applied to the index.
    fn update_settings<S: AsRef<str>>(&self, index_uid: S, settings: Settings) -> anyhow::Result<UpdateStatus>;

    /// Create an index with the given `index_uid`.
    fn create_index(&self, index_settings: IndexSettings) -> Result<IndexMetadata>;

    /// Delete index with the given `index_uid`, attempting to close it beforehand.
    fn delete_index<S: AsRef<str>>(&self, index_uid: S) -> Result<()>;

    /// Swap two indexes, concretely, it simply swaps the index the names point to.
    fn swap_indices<S1: AsRef<str>, S2: AsRef<str>>(&self, index1_uid: S1, index2_uid: S2) -> Result<()>;

    /// Apply an update to the given index. This method can be called when an update is ready to be
    /// processed
    fn handle_update<S: AsRef<str>>(
        &self,
        _index: S,
        _update_id: u64,
        _meta: Processing<UpdateMeta>,
        _content: &[u8]
    ) -> Result<Processed<UpdateMeta, UpdateResult>, Failed<UpdateMeta, String>> {
        todo!()
    }

    /// Returns, if it exists, the `Index` with the povided name.
    fn index(&self, name: impl AsRef<str>) -> anyhow::Result<Option<Arc<Index>>>;

    /// Returns the udpate status an update
    fn update_status(&self, index: impl AsRef<str>, id: u64) -> anyhow::Result<Option<UpdateStatus>>;

    /// Returns all the udpate status for an index
    fn all_update_status(&self, index: impl AsRef<str>) -> anyhow::Result<Vec<UpdateStatus>>;

    /// List all the indexes
    fn list_indexes(&self) -> anyhow::Result<Vec<IndexMetadata>>;

    fn update_index(&self, name: impl AsRef<str>, index_settings: IndexSettings) -> anyhow::Result<IndexMetadata>;
}


#[cfg(test)]
#[macro_use]
pub(crate) mod test {
    use super::*;

    #[macro_export]
    macro_rules! make_index_controller_tests {
        ($controller_buider:block) => {
            #[test]
            fn test_create_and_list_indexes() {
                crate::index_controller::test::create_and_list_indexes($controller_buider);
            }

            #[test]
            fn test_create_index_with_no_name_is_error() {
                crate::index_controller::test::create_index_with_no_name_is_error($controller_buider);
            }

            #[test]
            fn test_update_index() {
                crate::index_controller::test::update_index($controller_buider);
            }
        };
    }

    pub(crate) fn create_and_list_indexes(controller: impl IndexController) {
        let settings1 = IndexSettings {
            name: Some(String::from("test_index")),
            primary_key: None,
        };

        let settings2 = IndexSettings {
            name: Some(String::from("test_index2")),
            primary_key: Some(String::from("foo")),
        };

        controller.create_index(settings1).unwrap();
        controller.create_index(settings2).unwrap();

        let indexes = controller.list_indexes().unwrap();
        assert_eq!(indexes.len(), 2);
        assert_eq!(indexes[0].name, "test_index");
        assert_eq!(indexes[1].name, "test_index2");
        assert_eq!(indexes[1].primary_key.clone().unwrap(), "foo");
    }

    pub(crate) fn create_index_with_no_name_is_error(controller: impl IndexController) {
        let settings = IndexSettings {
            name: None,
            primary_key: None,
        };
        assert!(controller.create_index(settings).is_err());
    }

    pub(crate) fn update_index(controller: impl IndexController) {

        let settings = IndexSettings {
            name: Some(String::from("test")),
            primary_key: None,
        };

        assert!(controller.create_index(settings).is_ok());

        // perform empty update returns index meta unchanged
        let settings = IndexSettings {
            name: None,
            primary_key: None,
        };

        let result = controller.update_index("test", settings).unwrap();
        assert_eq!(result.name, "test");
        assert_eq!(result.created_at, result.updated_at);
        assert!(result.primary_key.is_none());

        // Changing the name trigger an error
        let settings = IndexSettings {
            name: Some(String::from("bar")),
            primary_key: None,
        };

        assert!(controller.update_index("test", settings).is_err());

        // Update primary key
        let settings = IndexSettings {
            name: None,
            primary_key: Some(String::from("foo")),
        };

        let result = controller.update_index("test", settings.clone()).unwrap();
        assert_eq!(result.name, "test");
        assert!(result.created_at < result.updated_at);
        assert_eq!(result.primary_key.unwrap(), "foo");

        // setting the primary key again is an error
        assert!(controller.update_index("test", settings).is_err());
    }
}
