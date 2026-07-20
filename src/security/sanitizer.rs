pub trait GritSanitizable {
    /// Mutates the struct fields in-place to cleanse XSS, format strings, and whitespace.
    fn sanitize(&mut self);
}

// Blanket default implementation so structs without macro decoration don't fail trait bounds
impl GritSanitizable for () {
    fn sanitize(&mut self) {}
}

// Sanitize Option<T> if it contains a value
impl<T: GritSanitizable> GritSanitizable for Option<T> {
    fn sanitize(&mut self) {
        if let Some(inner) = self {
            inner.sanitize();
        }
    }
}

// Sanitize every element in a Vec<T>
impl<T: GritSanitizable> GritSanitizable for Vec<T> {
    fn sanitize(&mut self) {
        for item in self.iter_mut() {
            item.sanitize();
        }
    }
}

// String default implementation if primitive types pass through
impl GritSanitizable for String {
    fn sanitize(&mut self) {}
}