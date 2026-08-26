use std::collections::BTreeMap;

use super::Value;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ValueStream {
    values: Vec<Value>,
}

impl ValueStream {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_values(values: Vec<Value>) -> Self {
        Self {
            values: values
                .into_iter()
                .flat_map(|value| match value {
                    Value::List(values) => values,
                    value => vec![value],
                })
                .collect(),
        }
    }

    pub fn from_record(fields: BTreeMap<String, Value>) -> Self {
        Self {
            values: vec![Value::Record(fields)],
        }
    }

    pub fn from_text_lines(values: Vec<Value>) -> Self {
        Self { values }
    }

    pub fn take(mut self, count: usize) -> Self {
        self.values.truncate(count);
        self
    }

    pub fn count(self) -> Result<Self, usize> {
        i64::try_from(self.values.len())
            .map(|count| Self::from_text_lines(vec![Value::Int(count)]))
            .map_err(|_| self.values.len())
    }

    pub fn collect(self) -> Self {
        Self::from_text_lines(vec![Value::List(self.values)])
    }

    pub fn into_values(self) -> Vec<Value> {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_expand_lists_by_one_level() {
        let stream = ValueStream::from_values(vec![
            Value::Int(1),
            Value::List(vec![Value::Int(2), Value::Int(3)]),
        ]);

        assert_eq!(
            stream.into_values(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn native_transformations_preserve_value_semantics() {
        let stream = ValueStream::from_values(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
            .take(2)
            .collect();

        assert_eq!(
            stream.into_values(),
            vec![Value::List(vec![Value::Int(1), Value::Int(2)])]
        );
    }
}
