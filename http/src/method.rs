use std::fmt;
use std::str::FromStr;

use crate::cmp_lower_case;

/// HTTP method.
///
/// RFC 7231 section 4.
#[derive(Copy, Clone, Debug)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
    /// RFC 5789.
    Patch,
}

impl Method {
    /// Returns `true` if `self` is a HEAD method.
    pub const fn is_head(self) -> bool {
        matches!(self, Method::Head)
    }

    /// Returns `true` if the method is safe.
    ///
    /// RFC 7321 section 4.2.1.
    pub const fn is_safe(self) -> bool {
        use Method::*;
        matches!(self, Get | Head | Options | Trace)
    }

    /// Returns `true` if the method is idempotent.
    ///
    /// RFC 7321 section 4.2.2.
    pub const fn is_idempotent(self) -> bool {
        matches!(self, Method::Put | Method::Delete) || self.is_safe()
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Method::*;
        f.write_str(match self {
            Options => "OPTIONS",
            Get => "GET",
            Post => "POST",
            Put => "PUT",
            Delete => "DELETE",
            Head => "HEAD",
            Trace => "TRACE",
            Connect => "CONNECT",
            Patch => "PATCH",
        })
    }
}

/// Error returned by the [`FromStr`] implementation for [`Method`].
#[derive(Copy, Clone, Debug)]
pub struct UnknownMethod;

impl FromStr for Method {
    type Err = UnknownMethod;

    fn from_str(method: &str) -> Result<Self, Self::Err> {
        match method.len() {
            3 => {
                if cmp_lower_case("get", method) {
                    Ok(Method::Get)
                } else if cmp_lower_case("put", method) {
                    Ok(Method::Put)
                } else {
                    Err(UnknownMethod)
                }
            }
            4 => {
                if cmp_lower_case("head", method) {
                    Ok(Method::Head)
                } else if cmp_lower_case("post", method) {
                    Ok(Method::Post)
                } else {
                    Err(UnknownMethod)
                }
            }
            5 => {
                if cmp_lower_case("trace", method) {
                    Ok(Method::Trace)
                } else if cmp_lower_case("patch", method) {
                    Ok(Method::Patch)
                } else {
                    Err(UnknownMethod)
                }
            }
            6 => {
                if cmp_lower_case("delete", method) {
                    Ok(Method::Delete)
                } else {
                    Err(UnknownMethod)
                }
            }
            7 => {
                if cmp_lower_case("connect", method) {
                    Ok(Method::Connect)
                } else if cmp_lower_case("options", method) {
                    Ok(Method::Options)
                } else {
                    Err(UnknownMethod)
                }
            }
            _ => Err(UnknownMethod),
        }
    }
}