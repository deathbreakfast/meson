use valence::prelude::*;
use valence::privacy_policies::common::{AUTHENTICATED, SYSTEM_ONLY};

/// Teaching File schema for Meson create/load examples (not a product table).
valence_schema! {
    ReceiptScan {
        repository: "https://github.com/unified-field-dev/meson",
        table: "receipt_scan",
        version: "0.1.0",
        database: crate::embedded_surreal::DEFAULT_STORAGE,
        description: "Uploaded receipt image (Meson teaching example)",

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
