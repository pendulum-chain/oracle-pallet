use std::fmt;

#[derive(Debug)]
pub enum ApiError {
    CoingeckoError(CoingeckoError),
    CustomError(CustomError),
    PolygonError(PolygonError),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiError::CoingeckoError(e) | ApiError::CustomError(e) | ApiError::PolygonError(e) => {
                write!(f, "{}", e)
            }
        }
    }
}

#[derive(Debug)]
pub struct CoingeckoError(pub String);

impl fmt::Display for CoingeckoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let CoingeckoError(ref err_msg) = *self;
        // Log the error message
        log::error!("CoinGeckoError: {}", err_msg);
        // Write the error message to the formatter
        write!(f, "{}", err_msg)
    }
}

#[derive(Debug)]
pub struct CustomError(pub String);

impl fmt::Display for CustomError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let CustomError(ref err_msg) = *self;
        // Log the error message
        log::error!("CustomError: {}", err_msg);
        // Write the error message to the formatter
        write!(f, "{}", err_msg)
    }
}

#[derive(Debug)]
pub struct PolygonError(pub String);

impl fmt::Display for PolygonError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let PolygonError(ref err_msg) = *self;
        // Log the error message
        log::error!("PolygonError: {}", err_msg);
        // Write the error message to the formatter
        write!(f, "{}", err_msg)
    }
}
