use axum::http::StatusCode;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateNameRequest {
    pub name: String,
}

impl CreateNameRequest {
    pub fn validated_name(&self) -> Result<&str, (StatusCode, &'static str)> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 255 || name.chars().any(char::is_control) {
            return Err((
                StatusCode::BAD_REQUEST,
                "Name must contain 1 to 255 characters without control characters.",
            ));
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_trimmed_unicode_names_and_rejects_blank_long_or_control_names() {
        let request = CreateNameRequest {
            name: "  Research lab  ".into(),
        };
        assert_eq!(request.validated_name().unwrap(), "Research lab");
        assert!(
            CreateNameRequest {
                name: "é".repeat(255)
            }
            .validated_name()
            .is_ok()
        );
        for name in [
            "".into(),
            " \t\n ".into(),
            "a".repeat(256),
            "Lab\nname".into(),
            "Lab\0name".into(),
        ] {
            assert!(CreateNameRequest { name }.validated_name().is_err());
        }
    }
}
