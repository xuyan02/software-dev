use crate::ValueError;
use std::fmt;

/// A compiled path through objects and arrays.
///
/// Syntax: dot-separated segments. Numeric segments address array elements.
///
/// ```text
/// task.title
/// nodes.load.output.items.0
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    segments: Box<[PathSegment]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    Key(Box<str>),
    Index(usize),
}

impl Path {
    pub fn parse(input: &str) -> Result<Self, ValueError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(ValueError::InvalidPath(input.to_owned()));
        }

        let mut segments = Vec::new();
        for raw in input.split('.') {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(ValueError::InvalidPath(input.to_owned()));
            }

            if raw.bytes().all(|byte| byte.is_ascii_digit()) {
                let index = raw
                    .parse::<usize>()
                    .map_err(|_| ValueError::InvalidPath(input.to_owned()))?;
                segments.push(PathSegment::Index(index));
            } else if is_identifier(raw) {
                segments.push(PathSegment::Key(raw.into()));
            } else {
                return Err(ValueError::InvalidPath(input.to_owned()));
            }
        }

        Ok(Self {
            segments: segments.into_boxed_slice(),
        })
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            match segment {
                PathSegment::Key(key) => f.write_str(key)?,
                PathSegment::Index(index) => write!(f, "{index}")?,
            }
        }
        Ok(())
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }

    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
