use valence::prelude::*;
use valence::privacy_policies::common::{AUTHENTICATED, SYSTEM_ONLY};

valence_schema! {
    E2eMesonFile {
        repository: "https://github.com/unified-field-dev/meson",
        table: "e2e_meson_file",
        version: "0.1.0",
        database: crate::embedded_surreal::DEFAULT_STORAGE,
        description: "Meson integration fixture implementing File (tests only)",

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
                allow: [SYSTEM_ONLY],
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
