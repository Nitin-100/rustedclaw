//! Weather lookup tool — stub that returns mock weather data.
//!
//! In production this would call a real weather API (OpenWeatherMap, etc.).
//! The stub returns plausible weather data so the agent loop and ReAct
//! pattern can be tested end-to-end without network access.

use async_trait::async_trait;
use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

pub struct WeatherLookupTool;

#[async_trait]
impl Tool for WeatherLookupTool {
    fn name(&self) -> &str {
        "weather_lookup"
    }

    fn description(&self) -> &str {
        "Look up current weather conditions for a location. Returns temperature, conditions, humidity, and wind speed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "location": {
                    "type": "string",
                    "description": "The city name or location to look up weather for"
                },
                "units": {
                    "type": "string",
                    "enum": ["metric", "imperial"],
                    "description": "Temperature units (default: metric)",
                    "default": "metric"
                }
            },
            "required": ["location"]
        })
    }

    async fn execute(&self, arguments: serde_json::Value) -> Result<ToolResult, ToolError> {
        let location = arguments["location"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArguments("Missing 'location' argument".into()))?;

        let units = arguments["units"].as_str().unwrap_or("metric");
        let weather = generate_mock_weather(location, units);
        let output = serde_json::to_string_pretty(&weather).unwrap_or_default();

        Ok(ToolResult {
            call_id: String::new(),
            success: true,
            output,
            data: Some(serde_json::to_value(&weather).unwrap()),
        })
    }
}

#[derive(serde::Serialize)]
struct WeatherData {
    location: String,
    temperature: f64,
    units: String,
    conditions: String,
    humidity: u32,
    wind_speed: f64,
    wind_direction: String,
}

/// Generate deterministic mock weather based on location name hash.
fn generate_mock_weather(location: &str, units: &str) -> WeatherData {
    // Simple hash for deterministic but varied results.
    let hash: u32 = location
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    let conditions_list = [
        "Clear skies",
        "Partly cloudy",
        "Overcast",
        "Light rain",
        "Heavy rain",
        "Thunderstorms",
        "Snow",
        "Foggy",
    ];

    let wind_dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

    let base_temp_c = ((hash % 40) as f64) - 5.0; // -5 to 35°C
    let (temperature, unit_label) = if units == "imperial" {
        (base_temp_c * 9.0 / 5.0 + 32.0, "°F")
    } else {
        (base_temp_c, "°C")
    };

    WeatherData {
        location: location.to_string(),
        temperature: (temperature * 10.0).round() / 10.0,
        units: unit_label.to_string(),
        conditions: conditions_list[(hash as usize / 7) % conditions_list.len()].to_string(),
        humidity: 30 + (hash % 60),
        wind_speed: ((hash % 30) as f64) + 5.0,
        wind_direction: wind_dirs[(hash as usize / 3) % wind_dirs.len()].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lookup_returns_weather() {
        let tool = WeatherLookupTool;
        let result = tool
            .execute(serde_json::json!({"location": "Tokyo"}))
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.output.contains("Tokyo"));
        assert!(result.output.contains("temperature"));
        assert!(result.data.is_some());
    }

    #[tokio::test]
    async fn imperial_units() {
        let tool = WeatherLookupTool;
        let result = tool
            .execute(serde_json::json!({"location": "New York", "units": "imperial"}))
            .await
            .unwrap();

        assert!(result.output.contains("°F"));
    }

    #[tokio::test]
    async fn deterministic_results() {
        let tool = WeatherLookupTool;
        let r1 = tool
            .execute(serde_json::json!({"location": "London"}))
            .await
            .unwrap();
        let r2 = tool
            .execute(serde_json::json!({"location": "London"}))
            .await
            .unwrap();

        assert_eq!(r1.output, r2.output);
    }

    #[tokio::test]
    async fn missing_location_returns_error() {
        let tool = WeatherLookupTool;
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn tool_definition() {
        let tool = WeatherLookupTool;
        let def = tool.to_definition();
        assert_eq!(def.name, "weather_lookup");
    }
}
