use std::convert::Infallible;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::str::FromStr;
use std::{fmt, fs};

use basic_toml as toml;
use eyre::WrapErr;
use log::{debug, warn};
use serde::{de, Deserialize, Deserializer};
use simple_eyre::eyre;
use time::format_description::OwnedFormatItem;
use time::{Date, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub rsspls: RssplsConfig,
    pub feed: Vec<ChannelConfig>,
}

#[derive(Debug, Deserialize)]
pub struct RssplsConfig {
    pub output: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChannelConfig {
    pub title: String,
    pub filename: String,
    pub user_agent: Option<String>,
    pub config: FeedConfig,
}

// TODO: Rename?
#[derive(Debug, Deserialize)]
pub struct FeedConfig {
    pub url: String,
    pub item: String,
    pub heading: String,
    pub link: Option<String>,
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "opt_string_or_struct")]
    pub date: Option<DateConfig>,
    pub media: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DateConfig {
    pub selector: String,
    #[serde(rename = "type", default)]
    type_: DateType,
    #[serde(deserialize_with = "deserialize_format")]
    pub format: Option<OwnedFormatItem>,
}

#[derive(Debug, Deserialize, Copy, Clone)]
enum DateType {
    Date,
    DateTime,
}

impl Default for DateType {
    fn default() -> Self {
        DateType::DateTime
    }
}

impl Config {
    /// Read the config file path and the supplied path or default if None
    pub fn read(config_path: Option<PathBuf>) -> eyre::Result<Config> {
        let dirs = crate::dirs::new()?;
        let config_path = config_path.ok_or(()).or_else(|()| {
            dirs.place_config_file("feeds.toml")
                .wrap_err("unable to create path to config file")
        })?;
        let raw_config = fs::read(&config_path).wrap_err_with(|| {
            format!(
                "unable to read configuration file: {}",
                config_path.display()
            )
        })?;
        toml::from_slice(&raw_config).wrap_err_with(|| {
            format!(
                "unable to parse configuration file: {}",
                config_path.display()
            )
        })
    }
}

impl DateConfig {
    pub fn selector(&self) -> &str {
        &self.selector
    }

    pub fn parse(&self, date: &str) -> eyre::Result<OffsetDateTime> {
        match self {
            DateConfig { format: None, .. } => {
                debug!("attempting to parse {} with anydate", date);
                anydate::parse(date)
                    .map(|chrono| {
                        // Convert chrono DateTime<FixedOffset> to time OffsetDateTime
                        OffsetDateTime::from_unix_timestamp(chrono.timestamp())
                            .unwrap()
                            .to_offset(
                                UtcOffset::from_whole_seconds(chrono.timezone().local_minus_utc())
                                    .unwrap(),
                            )
                    })
                    .map_err(eyre::Report::from)
            }
            DateConfig {
                format: Some(format),
                ..
            } => {
                debug!("attempting to parse {} with supplied format", date);
                match self.type_ {
                    DateType::Date => Date::parse(date, format)
                        .map(|date| PrimitiveDateTime::new(date, Time::MIDNIGHT).assume_utc())
                        .map_err(|err| {
                            debug!("parsing with format failed: {}", err);
                            eyre::Report::from(err)
                        }),
                    DateType::DateTime => OffsetDateTime::parse(date, format)
                        .or_else(|_| {
                            PrimitiveDateTime::parse(date, format)
                                .map(|primitive| primitive.assume_utc())
                        })
                        .map_err(|err| {
                            debug!("parsing with format failed: {}", err);
                            eyre::Report::from(err)
                        }),
                }
            }
        }
    }
}

impl FromStr for DateConfig {
    // This implementation of `from_str` can never fail, so use the
    // `Infallible` type as the error type.
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(DateConfig {
            selector: s.to_string(),
            ..Default::default()
        })
    }
}

pub fn deserialize_format<'de, D>(deserializer: D) -> Result<Option<OwnedFormatItem>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    s.map(|s| time::format_description::parse_owned::<2>(&s))
        .transpose()
        .map_err(|err| {
            warn!("unable to parse date format: {}", err);
            serde::de::Error::custom(err)
        })
}

// https://serde.rs/string-or-struct.html
fn string_or_struct<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = Infallible>,
    D: Deserializer<'de>,
{
    // This is a Visitor that forwards string types to T's `FromStr` impl and
    // forwards map types to T's `Deserialize` impl. The `PhantomData` is to
    // keep the compiler from complaining about T being an unused generic type
    // parameter. We need T in order to know the Value type for the Visitor
    // impl.
    struct StringOrStruct<T>(PhantomData<fn() -> T>);

    impl<'de, T> de::Visitor<'de> for StringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = Infallible>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or map")
        }

        fn visit_str<E>(self, value: &str) -> Result<T, E>
        where
            E: de::Error,
        {
            Ok(FromStr::from_str(value).unwrap())
        }

        fn visit_map<M>(self, map: M) -> Result<T, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            // `MapAccessDeserializer` is a wrapper that turns a `MapAccess`
            // into a `Deserializer`, allowing it to be used as the input to T's
            // `Deserialize` implementation. T then deserializes itself using
            // the entries from the map visitor.
            Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(StringOrStruct(PhantomData))
}

// https://github.com/emk/compose_yml/blob/7e8e0f47dcc41cf08e15fe082ef4c40b5f0475eb/src/v2/string_or_struct.rs#L69
pub fn opt_string_or_struct<'de, T, D>(d: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = Infallible>,
    D: Deserializer<'de>,
{
    /// Declare an internal visitor type to handle our input.
    struct OptStringOrStruct<T>(PhantomData<T>);

    impl<'de, T> de::Visitor<'de> for OptStringOrStruct<T>
    where
        T: Deserialize<'de> + FromStr<Err = Infallible>,
    {
        type Value = Option<T>;

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            string_or_struct(deserializer).map(Some)
        }

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a null, a string or a map")
        }
    }

    d.deserialize_option(OptStringOrStruct(PhantomData))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_date(format: &'static str) -> DateConfig {
        DateConfig {
            selector: String::new(),
            type_: DateType::Date,
            format: Some(time::format_description::parse_owned::<2>(format).unwrap()),
        }
    }

    fn test_anydate() -> DateConfig {
        DateConfig {
            selector: String::new(),
            type_: DateType::Date,
            format: None,
        }
    }

    #[test]
    fn test_without_format() {
        assert!(test_anydate().parse("January 8, 2021").is_ok());
        assert!(test_anydate().parse("2022-07-13").is_ok());
        assert!(test_anydate().parse("12/31/1999").is_ok());
    }

    #[test]
    fn test_with_date_format() {
        assert!(test_date("[day padding:none]/[month padding:none]/[year]")
            .parse("1/2/1945")
            .is_ok());
        assert!(test_date("[weekday case_sensitive:false], [month repr:long case_sensitive:false] [day padding:none][first [st][nd][rd][th]], [year]")
            .parse("Friday, January 8th, 2021").is_ok());
        assert!(test_date("[weekday case_sensitive:false], [month repr:long case_sensitive:false] [day padding:none], [year]")
            .parse("Friday, January 8, 2021").is_ok());
    }

    #[test]
    fn test_with_date_time_format() {
        assert!(test_date("[weekday case_sensitive:false], [month repr:long case_sensitive:false] [day padding:none][first [st][nd][rd][th]], [year] [hour repr:12]:[minute][period case:lower]")
            .parse("Friday, January 8th, 2021 12:13pm").is_ok());
        assert!(test_date("[weekday case_sensitive:false], [month repr:long case_sensitive:false] [day padding:none], [year] [hour repr:24]:[minute]")
            .parse("Friday, January 8, 2021 21:33").is_ok());
    }
}
