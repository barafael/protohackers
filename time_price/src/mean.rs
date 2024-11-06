pub trait Mean {
    fn mean(self) -> f64;
}

impl<F, T> Mean for T
where
    T: Iterator<Item = F>,
    F: std::borrow::Borrow<i32>,
{
    fn mean(self) -> f64 {
        self.zip(1..).fold(0., |s, (e, i)| {
            (f64::from(*e.borrow()) + s * f64::from(i - 1)) / f64::from(i)
        })
    }
}

#[cfg(test)]
mod test {
    use crate::mean::Mean;

    #[test]
    fn means() {
        assert_eq!([1, 2, 3, 4, 5].iter().mean(), 3.);
        assert_eq!([1, 2, 3, 4, 5].iter().mean(), 3.);
        assert_eq!(vec![1, 2, 3, 4, 5].into_iter().mean(), 3.);
    }
}
