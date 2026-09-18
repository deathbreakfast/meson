use valence::prelude::*;
use valence::privacy_policies::common::{AUTHENTICATED, SYSTEM_ONLY};

/// Second File-bearing fixture (ProfilePhoto-shaped) so FileQueryAll union is covered.
valence_schema! {
    E2eMesonProfilePhoto {
        repository: "https://github.com/unified-field-dev/meson",
        table: "e2e_meson_profile_photo",
        version: "0.1.0",
        database: crate::embedded_surreal::DEFAULT_STORAGE,
        description: "Meson ProfilePhoto-as-File fixture (tests only)",

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
            width: {
                r#type: FieldType::Integer,
                required: false,
            },
            height: {
                r#type: FieldType::Integer,
                required: false,
            },
        ],
    }
}
