use valence::prelude::*;
use valence::privacy_policies::common::{AUTHENTICATED, SYSTEM_ONLY};

/// File fixture whose owner may delete their own rows (tests only).
valence_schema! {
    E2eMesonOwnedFile {
        repository: "https://github.com/unified-field-dev/meson",
        table: "e2e_meson_owned_file",
        version: "0.1.0",
        database: crate::embedded_surreal::DEFAULT_STORAGE,
        description: "Meson File fixture an uploader can delete (tests only)",

        traits: [File],

        policies: {
            read: {
                allow: [AUTHENTICATED],
            },
            create: {
                allow: [SYSTEM_ONLY],
            },
            update: {
                allow: [SYSTEM_ONLY],
            },
            delete: {
                allow: [crate::delete::UPLOADED_BY_OWNER],
            },
        },

        fields: [
            id: {
                r#type: FieldType::String,
                primary_key: true,
                required: true,
            },
        ],
    }
}
