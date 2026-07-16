//! Remote chat projection helpers owned by the remote-connect integration.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bitfun_runtime_ports::AgentInputAttachment;
use image::imageops::FilterType;

use super::{ChatImageAttachment, RemoteImageContext};

/// Max thumbnail size per remote chat image sent to mobile (100 KB).
const REMOTE_CHAT_MOBILE_IMAGE_MAX_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChatUserProjection {
    pub content: String,
    pub images: Vec<ChatImageAttachment>,
}

pub fn project_remote_chat_user(
    metadata: Option<&serde_json::Value>,
    prompt_visible_content: &str,
) -> RemoteChatUserProjection {
    let original_text = metadata
        .and_then(|metadata| metadata.get("original_text"))
        .and_then(|value| value.as_str());

    RemoteChatUserProjection {
        content: remote_chat_user_display_content(original_text, prompt_visible_content),
        images: remote_chat_user_images_from_metadata(metadata),
    }
}

/// Compress a base64 data-URL image to a small thumbnail for mobile display.
/// Falls back to the original if decoding/compression fails or the image is
/// already within the mobile thumbnail budget.
fn compress_remote_chat_data_url_for_mobile(data_url: &str) -> String {
    const MAX_THUMBNAIL_DIM: u32 = 400;

    let Some(comma_pos) = data_url.find(',') else {
        return data_url.to_string();
    };
    let b64_data = &data_url[comma_pos + 1..];

    if b64_data.len() * 3 / 4 <= REMOTE_CHAT_MOBILE_IMAGE_MAX_BYTES {
        return data_url.to_string();
    }

    let Ok(raw_bytes) = BASE64.decode(b64_data) else {
        return data_url.to_string();
    };

    let Ok(img) = image::load_from_memory(&raw_bytes) else {
        return data_url.to_string();
    };

    let resized = if img.width() > MAX_THUMBNAIL_DIM || img.height() > MAX_THUMBNAIL_DIM {
        img.resize(MAX_THUMBNAIL_DIM, MAX_THUMBNAIL_DIM, FilterType::Triangle)
    } else {
        img
    };

    fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        img.write_with_encoder(encoder).ok()?;
        Some(buf)
    }

    for quality in [75u8, 60, 45, 30] {
        if let Some(buf) = encode_jpeg(&resized, quality) {
            if buf.len() <= REMOTE_CHAT_MOBILE_IMAGE_MAX_BYTES || quality == 30 {
                let b64 = BASE64.encode(&buf);
                return format!("data:image/jpeg;base64,{b64}");
            }
        }
    }

    data_url.to_string()
}

fn safe_remote_chat_image(name: &str, data_url: &str) -> Option<ChatImageAttachment> {
    const MAX_DATA_URL_LENGTH: usize = 2 * 1024 * 1024;

    if data_url.len() > MAX_DATA_URL_LENGTH {
        return None;
    }
    let comma_pos = data_url.find(',')?;
    let header = data_url[..comma_pos].to_ascii_lowercase();
    if !matches!(
        header.as_str(),
        "data:image/png;base64"
            | "data:image/jpeg;base64"
            | "data:image/jpg;base64"
            | "data:image/webp;base64"
            | "data:image/gif;base64"
    ) {
        return None;
    }
    let encoded = &data_url[comma_pos + 1..];
    if encoded.is_empty() || BASE64.decode(encoded).is_err() {
        return None;
    }

    let safe_name = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("image")
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect::<String>();
    Some(ChatImageAttachment {
        name: if safe_name.trim().is_empty() {
            "image".to_string()
        } else {
            safe_name
        },
        data_url: compress_remote_chat_data_url_for_mobile(data_url),
    })
}

fn remote_chat_user_images_from_metadata(
    metadata: Option<&serde_json::Value>,
) -> Vec<ChatImageAttachment> {
    metadata
        .and_then(|metadata| metadata.get("images"))
        .and_then(|value| value.as_array())
        .map(|images| {
            images
                .iter()
                .filter_map(|image| {
                    let name = image.get("name")?.as_str()?;
                    let raw_url = image.get("data_url")?.as_str()?;
                    safe_remote_chat_image(name, raw_url)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn remote_chat_user_display_content(
    original_text: Option<&str>,
    prompt_visible_content: &str,
) -> String {
    if let Some(original_text) = original_text.filter(|value| !value.trim().is_empty()) {
        return original_text.to_string();
    }

    if prompt_visible_content.starts_with("User uploaded") {
        if let Some(pos) = prompt_visible_content.find("User's question:\n") {
            return prompt_visible_content[pos + "User's question:\n".len()..]
                .trim()
                .to_string();
        }
    }

    prompt_visible_content.to_string()
}

pub fn agent_input_attachment_from_remote_image_context(
    context: RemoteImageContext,
) -> AgentInputAttachment {
    let mut metadata = serde_json::Map::new();
    if let Some(image_path) = context.image_path {
        metadata.insert(
            "imagePath".to_string(),
            serde_json::Value::String(image_path),
        );
    }
    if let Some(data_url) = context.data_url {
        metadata.insert("dataUrl".to_string(), serde_json::Value::String(data_url));
    }
    metadata.insert(
        "mimeType".to_string(),
        serde_json::Value::String(context.mime_type),
    );
    if let Some(context_metadata) = context.metadata {
        metadata.insert("metadata".to_string(), context_metadata);
    }

    AgentInputAttachment {
        kind: "remote_image".to_string(),
        id: context.id,
        metadata,
    }
}
