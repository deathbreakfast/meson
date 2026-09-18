//! [`MesonImg`] — Orbital Image gated on Meson File virus-scan status.

use leptos::prelude::*;
use orbital_core_components::SkeletonItemShape;
use orbital_primitives::{Image, ImageConfig, ImageFit, ImageShape, Skeleton, SkeletonItem};

use crate::server::{get_meson_image_src, subscribe_meson_file_live_sync};
use crate::status::is_pending_status;

/// File-id image that syncs until Available, then serves `/api/files/{id}`.
///
/// Pending → Orbital Skeleton sized to `width`/`height`. Available → Orbital
/// Image. Quarantined / unauthorized → empty framed space (no `src`).
#[component]
pub fn MesonImg(
    /// Valence File id (`table:uuid`) or bare UUID.
    #[prop(into)]
    file_id: String,
    /// Accessible description passed to Orbital Image.
    #[prop(into)]
    alt: String,
    /// CSS width (e.g. `"120px"`).
    #[prop(optional, into)]
    width: Option<String>,
    /// CSS height.
    #[prop(optional, into)]
    height: Option<String>,
    /// Orbital image shape preset.
    #[prop(optional)]
    shape: Option<ImageShape>,
    /// Orbital object-fit preset.
    #[prop(optional)]
    fit: Option<ImageFit>,
) -> impl IntoView {
    let file_id_signal = RwSignal::new(file_id);
    let alt_signal = StoredValue::new(alt);
    let width_signal = StoredValue::new(width);
    let height_signal = StoredValue::new(height);
    let shape_signal = StoredValue::new(shape.unwrap_or_default());
    let fit_signal = StoredValue::new(fit.unwrap_or_default());

    let ws_trigger = subscribe_meson_file_live_sync(|| {});

    let src_res = Resource::new(
        move || (file_id_signal.get(), ws_trigger.get()),
        |(fid, _)| async move {
            if fid.is_empty() {
                return Err(ServerFnError::new("missing file_id"));
            }
            get_meson_image_src(fid).await
        },
    );

    view! {
        <span data-testid="meson-img">
            <Suspense fallback=move || {
                pending_skeleton(
                    width_signal.get_value(),
                    height_signal.get_value(),
                    shape_signal.get_value(),
                )
            }>
                {move || match src_res.get() {
                    Some(Ok(dto)) if is_pending_status(&dto.file_status) => {
                        pending_skeleton(
                            width_signal.get_value(),
                            height_signal.get_value(),
                            shape_signal.get_value(),
                        )
                        .into_any()
                    }
                    Some(Ok(dto)) => {
                        if let Some(src) = dto.src {
                            let mut config = ImageConfig::src(src, alt_signal.get_value());
                            if let Some(w) = width_signal.get_value() {
                                config.width = Some(w);
                            }
                            if let Some(h) = height_signal.get_value() {
                                config.height = Some(h);
                            }
                            config = config
                                .with_shape(shape_signal.get_value())
                                .with_fit(fit_signal.get_value());
                            view! { <Image config=config /> }.into_any()
                        } else {
                            empty_frame(
                                width_signal.get_value(),
                                height_signal.get_value(),
                            )
                            .into_any()
                        }
                    }
                    Some(Err(_)) => empty_frame(
                        width_signal.get_value(),
                        height_signal.get_value(),
                    )
                    .into_any(),
                    None => pending_skeleton(
                        width_signal.get_value(),
                        height_signal.get_value(),
                        shape_signal.get_value(),
                    )
                    .into_any(),
                }}
            </Suspense>
        </span>
    }
}

fn pending_skeleton(
    width: Option<String>,
    height: Option<String>,
    shape: ImageShape,
) -> impl IntoView {
    let item_shape = match shape {
        ImageShape::Circular => SkeletonItemShape::Circle,
        _ => SkeletonItemShape::Rectangle,
    };
    let w = width.unwrap_or_else(|| "100%".into());
    let h = height.unwrap_or_else(|| "120px".into());
    view! {
        <Skeleton>
            <SkeletonItem
                width=w
                height=h
                shape=Signal::from(item_shape)
            />
        </Skeleton>
    }
}

fn empty_frame(width: Option<String>, height: Option<String>) -> impl IntoView {
    // Sized empty Image frame (no src) — preserves layout without leaking bytes.
    let mut config = ImageConfig::default();
    config.alt = Some(String::new());
    config.width = width;
    config.height = height;
    view! { <Image config=config /> }
}
