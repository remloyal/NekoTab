//! 天气数据源（仅服务端）。
//!
//! 用 Open-Meteo。选它的唯一理由是**不需要 API key**：要注册账号、要配密钥的数据源
//! 会把"部署完就能看天气"变成"先去申请一个密钥再填进环境变量"，而那不是一个默认
//! 装好的组件该有的前置条件。换数据源只需改这个文件里的两个地址和两个解析函数。
//!
//! 两条与别处一致的约定：
//!
//! - 与 `suggest.rs` 一样，**解析函数对坏数据只许返回 `None`，不许 panic**（上游改
//!   字段、限流返回 HTML、断网返回半截，都只是"这次取不到"）。
//! - 与 `/icon/{host}` 不同，这里的地址是**写死**的：用户输入只进查询参数，而且已经
//!   百分号编码，所以不存在"让对方指定目标地址"那类问题。
//!
//! 只在服务端编译：浏览器直连这两个接口会被 CORS 挡，而且统一在服务端取也能
//! 用一个超时（见 `backend::http_client`）。

use crate::engine::encode_query;
use crate::models::Weather;

/// 地名 → 经纬度。`count=1` 只要最匹配的一条。
const GEOCODE_ENDPOINT: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// 当前天气 + 今日高低。`current` 与 `daily` 一起取，一次请求拿全。
const FORECAST_ENDPOINT: &str = "https://api.open-meteo.com/v1/forecast";

/// 地理编码地址。地名走百分号编码，中文与空格都能安全拼进去。
pub fn geocode_url(location: &str) -> String {
    format!(
        "{GEOCODE_ENDPOINT}?name={}&count=1&language=zh&format=json",
        encode_query(location)
    )
}

/// 预报地址。用 `timezone=auto`：让数据源按当地时区算"今天"，否则跨时区取到的
/// 最高/最低会是错的日期。`forecast_days=1` 只要今天。
pub fn forecast_url(latitude: f64, longitude: f64, fahrenheit: bool) -> String {
    let unit = if fahrenheit { "fahrenheit" } else { "celsius" };
    format!(
        "{FORECAST_ENDPOINT}?latitude={latitude}&longitude={longitude}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min\
         &timezone=auto&forecast_days=1&temperature_unit={unit}"
    )
}

/// 从地理编码响应里取出最匹配的一条：`(规范地名, 纬度, 经度)`。
///
/// 查不到时 Open-Meteo **根本不返回 `results` 字段**（不是返回一个空数组），所以这里
/// 整条路都按 `Option` 走：字段缺了、类型不对、JSON 坏了，一律 `None`。
pub fn parse_geocode(body: &str) -> Option<(String, f64, f64)> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let first = value.get("results")?.as_array()?.first()?;

    // 数字在 JSON 里可能是整数写法（`12` 而不是 `12.0`），serde_json 的 as_f64 两者都认
    Some((
        first.get("name")?.as_str()?.to_string(),
        first.get("latitude")?.as_f64()?,
        first.get("longitude")?.as_f64()?,
    ))
}

/// 从预报响应里取出 `(当前气温, 今日最高, 今日最低, 天气代码)`。
///
/// `daily` 下面几个字段都是**数组**（接口支持一次要多天），这里只要第一天，
/// 所以取下标 0。数组为空视为取不到。
pub fn parse_forecast(body: &str) -> Option<(f64, f64, f64, u32)> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;

    let current = value.get("current")?;
    let temperature = current.get("temperature_2m")?.as_f64()?;
    let code = current.get("weather_code")?.as_u64()? as u32;

    let daily = value.get("daily")?;
    let high = daily.get("temperature_2m_max")?.as_array()?.first()?.as_f64()?;
    let low = daily.get("temperature_2m_min")?.as_array()?.first()?.as_f64()?;

    Some((temperature, high, low, code))
}

/// 查一次天气。地点查不到时返回一句能直接摆到卡片上的话，而不是 Err 的调试形态。
pub async fn fetch(location: &str, fahrenheit: bool) -> Result<Weather, String> {
    let body = crate::backend::http_client()
        .get(geocode_url(location))
        .send()
        .await
        .map_err(|e| format!("查地点失败：{e}"))?
        .text()
        .await
        .map_err(|e| format!("读取地点响应失败：{e}"))?;

    let Some((place, latitude, longitude)) = parse_geocode(&body) else {
        return Err(format!("找不到「{location}」这个地方"));
    };

    let body = crate::backend::http_client()
        .get(forecast_url(latitude, longitude, fahrenheit))
        .send()
        .await
        .map_err(|e| format!("取天气失败：{e}"))?
        .text()
        .await
        .map_err(|e| format!("读取天气响应失败：{e}"))?;

    let Some((temperature, high, low, code)) = parse_forecast(&body) else {
        return Err("天气接口回了看不懂的内容".to_string());
    };

    Ok(Weather {
        place,
        temperature,
        high,
        low,
        code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实形状的地理编码响应（删掉了用不上的字段）。
    const GEOCODE_OK: &str = r#"{
        "results": [
            {
                "id": 1816670,
                "name": "北京市",
                "latitude": 39.9075,
                "longitude": 116.39723,
                "country": "中国",
                "admin1": "北京市"
            }
        ],
        "generationtime_ms": 1.2
    }"#;

    /// 真实形状的预报响应。
    const FORECAST_OK: &str = r#"{
        "latitude": 39.875,
        "longitude": 116.375,
        "timezone": "Asia/Shanghai",
        "current": {
            "time": "2026-10-15T22:00",
            "interval": 900,
            "temperature_2m": 12.4,
            "weather_code": 3
        },
        "daily": {
            "time": ["2026-10-15"],
            "temperature_2m_max": [16.3],
            "temperature_2m_min": [7.1]
        }
    }"#;

    #[test]
    fn geocode_url_encodes_the_place_name() {
        // 中文与空格都要被编码，否则拼出来的查询串是坏的
        let url = geocode_url("New York");
        assert!(url.contains("name=New%20York"), "实际：{url}");
        assert!(!url.contains(' '), "查询串里不该有裸空格");

        let url = geocode_url("北京");
        assert!(url.contains("name=%E5%8C%97%E4%BA%AC"), "实际：{url}");
    }

    #[test]
    fn forecast_url_carries_coordinates_and_unit() {
        let celsius = forecast_url(39.9075, 116.39723, false);
        assert!(celsius.contains("latitude=39.9075"));
        assert!(celsius.contains("longitude=116.39723"));
        assert!(celsius.contains("temperature_unit=celsius"));
        assert!(celsius.contains("timezone=auto"), "要按当地时区算今天");
        assert!(celsius.contains("current=temperature_2m,weather_code"));
        assert!(celsius.contains("daily=temperature_2m_max,temperature_2m_min"));

        assert!(forecast_url(1.0, 2.0, true).contains("temperature_unit=fahrenheit"));
    }

    #[test]
    fn geocode_reads_the_first_match() {
        assert_eq!(
            parse_geocode(GEOCODE_OK),
            Some(("北京市".to_string(), 39.9075, 116.39723))
        );
    }

    #[test]
    fn geocode_treats_a_missing_results_key_as_no_match() {
        // 查不到时接口不返回 results 字段——这正是最容易写崩的一处
        assert_eq!(parse_geocode(r#"{"generationtime_ms": 1.2}"#), None);
        // 空数组同样是查不到
        assert_eq!(parse_geocode(r#"{"results": []}"#), None);
        // 缺经纬度的那条不算数
        assert_eq!(parse_geocode(r#"{"results": [{"name": "某地"}]}"#), None);
    }

    #[test]
    fn forecast_reads_current_and_today() {
        assert_eq!(parse_forecast(FORECAST_OK), Some((12.4, 16.3, 7.1, 3)));
    }

    #[test]
    fn integer_temperatures_are_accepted() {
        // 接口给整数写法（`12` 而不是 `12.0`）时不能因为类型判断而丢掉
        let body = r#"{"current": {"temperature_2m": 12, "weather_code": 0},
                       "daily": {"temperature_2m_max": [16], "temperature_2m_min": [7]}}"#;
        assert_eq!(parse_forecast(body), Some((12.0, 16.0, 7.0, 0)));
    }

    #[test]
    fn bad_input_gives_none_not_panic() {
        let junk = [
            "",
            "not json",
            "[]",
            "null",
            "123",
            "<html>429 Too Many Requests</html>",
            r#"{"current": {}}"#,
            r#"{"current": {"temperature_2m": 12.4, "weather_code": 3}}"#,
            r#"{"current": {"temperature_2m": 12.4, "weather_code": 3}, "daily": {}}"#,
            r#"{"current": {"temperature_2m": 12.4, "weather_code": 3},
                "daily": {"temperature_2m_max": [], "temperature_2m_min": []}}"#,
            r#"{"current": {"temperature_2m": null, "weather_code": 3},
                "daily": {"temperature_2m_max": [16.0], "temperature_2m_min": [7.0]}}"#,
        ];

        for body in junk {
            assert_eq!(parse_forecast(body), None, "喂进 {body:?} 应得 None");
            assert_eq!(parse_geocode(body), None, "喂进 {body:?} 应得 None");
        }
    }
}
