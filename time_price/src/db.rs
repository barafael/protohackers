use crate::mean::Mean;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Db(BTreeMap<i32, i32>);

impl Db {
    pub fn insert(&mut self, time: i32, price: i32) {
        self.0.insert(time, price);
    }

    pub fn query(&self, min: i32, max: i32) -> i32 {
        let interval = min..=max;
        self.0
            .iter()
            .filter_map(|(k, v)| if interval.contains(k) { Some(v) } else { None })
            .mean() as i32
    }
}

#[cfg(test)]
mod test {
    use super::Db;

    #[test]
    fn insert_and_query() {
        let mut db = Db::default();
        db.insert(1, 2);
        db.insert(2, 3);
        db.insert(0, 4);
        assert_eq!(3, { db.query(0, 4) });
        assert_eq!(0, { db.query(10, 9) });
    }
}
