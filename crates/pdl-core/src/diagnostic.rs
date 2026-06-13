use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Severity, Span};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(&'static str);

impl DiagnosticCode {
    pub const fn new(code: &'static str) -> Self {
        Self(code)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }

    pub fn parse(code: &str) -> Option<Self> {
        all_codes()
            .iter()
            .copied()
            .find(|registered| registered.as_str() == code)
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl PartialEq<&str> for DiagnosticCode {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<DiagnosticCode> for &str {
    fn eq(&self, other: &DiagnosticCode) -> bool {
        *self == other.0
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0)
    }
}

impl<'de> Deserialize<'de> for DiagnosticCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = String::deserialize(deserializer)?;
        DiagnosticCode::parse(&code)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown diagnostic code `{code}`")))
    }
}

macro_rules! register_codes {
    ($($name:ident),+ $(,)?) => {
        pub mod codes {
            use super::DiagnosticCode;

            $(
                pub const $name: DiagnosticCode = DiagnosticCode::new(stringify!($name));
            )+
        }

        const REGISTERED_CODES: &[DiagnosticCode] = &[
            $(codes::$name,)+
        ];
    };
}

register_codes! {
    E0001, E0002, E0003, E0004, E0005, E0006, E0007, E0008,
    E0009, E0010, E0011, E0012, E0013, E0014, E0015, E0016,
    E0017, E0018, E0019, E0020, E0021, E0022, E0023, E0024,
    E0025, E0026, E0027,
    E1001, E1002, E1003, E1004, E1005, E1006, E1007, E1008,
    E1009, E1010, E1011, E1012, E1013, E1014, E1015,
    E1201, E1202, E1203, E1204, E1205, E1206, E1207, E1208,
    E1209, E1210, E1211, E1212, E1213, E1214, E1215, E1216,
    E1217, E1218, E1219, E1220, E1221, E1222, E1223, E1224,
    E1225, E1226, E1230, E1231, E1232, E1233, E1234,
    E1301, E1302, E1303, E1304, E1305, E1306, E1307, E1308,
    E1309, E1310, E1311, E1312, E1313,
    E1401, E1402, E1403, E1404, E1405, E1406, E1407, E1408,
    E1409, E1410, E1411, E1412, E1413, E1414, E1415, E1416,
    E1417,
    E1501, E1502, E1503, E1504, E1505, E1506, E1507, E1508,
    E1509, E1510,
    E1601, E1602, E1603, E1604, E1605, E1606, E1607, E1608,
    E1609, E1610, E1611,
    E1701, E1702, E1703, E1704, E1705, E1706, E1707, E1708,
    E1709, E1710, E1711, E1712,
    E1801, E1802, E1803, E1804, E1805, E1806, E1807, E1808,
    E1809, E1810, E1811, E1812, E1813, E1814, E1815, E1816,
    E1817, E1818, E1819, E1820, E1821, E1822,
    E1901, E1902, E1903, E1904, E1905,
    E2001, E2002, E2003, E2004, E2005, E2006, E2007, E2008,
    E2009, E2010, E2011, E2012, E2013,
    W2001, W2002, W2003, W2004, W2005, W2006, W2007, W2008,
    W2009, W2010, W2011, W2012,
    H3001, H3002, H3003, H3004, H3005, H3006, H3007, H3008,
    H3009, H3010, H3011, H3012,
    R4001, R4002, R4003, R4004, R4005, R4006, R4007, R4008,
}

pub const fn all_codes() -> &'static [DiagnosticCode] {
    REGISTERED_CODES
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RelatedSpan {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        code: DiagnosticCode,
        message: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            severity,
            code: code.as_str(),
            message: message.into(),
            span,
            related: Vec::new(),
            help: None,
        }
    }

    pub fn error(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(Severity::Error, code, message, span)
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(Severity::Warning, code, message, span)
    }

    pub fn info(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(Severity::Info, code, message, span)
    }

    pub fn hint(code: DiagnosticCode, message: impl Into<String>, span: Span) -> Self {
        Self::new(Severity::Hint, code, message, span)
    }

    pub fn with_related(mut self, span: Span, message: impl Into<String>) -> Self {
        self.related.push(RelatedSpan {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    #[test]
    fn registered_codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in all_codes() {
            let text = code.as_str();
            assert_eq!(text.len(), 5, "{text}");
            assert!(
                matches!(text.as_bytes()[0], b'E' | b'W' | b'H' | b'R'),
                "{text}"
            );
            assert!(
                text[1..].bytes().all(|byte| byte.is_ascii_digit()),
                "{text}"
            );
            assert!(seen.insert(text), "duplicate diagnostic code {text}");
            assert_eq!(DiagnosticCode::parse(text), Some(*code));
        }
    }

    #[test]
    fn diagnostic_payload_is_stable() {
        let diagnostic = Diagnostic::error(codes::E1005, "unknown column", Span::new(4, 6))
            .with_related(Span::new(0, 2), "source schema")
            .with_help("check the column spelling");

        assert_eq!(diagnostic.code, "E1005");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert_eq!(diagnostic.related[0].message, "source schema");
        assert_eq!(
            diagnostic.help.as_deref(),
            Some("check the column spelling")
        );
    }

    #[test]
    fn spec_diagnostic_codes_are_registered() {
        let spec = include_str!("../../../docs/PDL_SPEC.md");
        let documented = code_literals(spec);
        let registered: BTreeSet<&str> = all_codes().iter().map(|code| code.as_str()).collect();

        for code in &documented {
            assert!(
                registered.contains(code.as_str()),
                "{code} is documented but not registered"
            );
        }

        for code in all_codes() {
            assert!(
                documented.contains(code.as_str()),
                "{} is registered but not documented",
                code.as_str()
            );
        }
    }

    fn code_literals(text: &str) -> BTreeSet<String> {
        let mut codes = BTreeSet::new();
        let bytes = text.as_bytes();
        for index in 0..bytes.len().saturating_sub(4) {
            if matches!(bytes[index], b'E' | b'W' | b'H' | b'R')
                && bytes[index + 1..index + 5]
                    .iter()
                    .all(|byte| byte.is_ascii_digit())
            {
                let range_before = index >= 2 && bytes.get(index - 2) == Some(&b'-');
                let range_after = bytes.get(index + 6) == Some(&b'-');
                if range_before || range_after {
                    continue;
                }
                codes.insert(text[index..index + 5].to_string());
            }
        }
        codes
    }
}
