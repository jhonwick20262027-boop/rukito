//! Primitivas de firma compartidas por los tres adaptadores (§10.13).
//!
//! Todo es determinista y puro salvo la fecha actual, que se pasa como parámetro
//! para que los vectores de la suite U no dependan del reloj.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// SHA-256 en hexadecimal minúscula.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

/// SHA-256 del cuerpo vacío, que usan `GET`, `HEAD` y `DELETE`.
pub(crate) fn sha256_empty_hex() -> String {
    sha256_hex(b"")
}

/// HMAC-SHA256 crudo.
pub(crate) fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC acepta claves de cualquier tamaño");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// HMAC-SHA256 en hexadecimal minúscula.
#[cfg(any(feature = "aws", feature = "gcp"))]
pub(crate) fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

/// Cabeceras canónicas ordenadas y la lista `SignedHeaders`.
///
/// Los valores se recortan y las cabeceras se comparan en minúsculas, como piden
/// SigV4 y V4 de GCS.
pub(crate) fn canonical_headers(headers: &[(String, String)]) -> (String, String) {
    let mut headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    headers.sort_by(|left, right| left.0.cmp(&right.0));
    let mut canonical = String::new();
    let mut signed = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        canonical.push_str(&name);
        canonical.push(':');
        canonical.push_str(&value);
        canonical.push('\n');
        signed.push(name);
    }
    (canonical, signed.join(";"))
}

/// Petición canónica de SigV4/V4 (sin query en esta fase).
pub(crate) fn canonical_request(
    method: &str,
    uri: &str,
    headers: &[(String, String)],
    payload_hash: &str,
) -> (String, String) {
    let (headers, signed) = canonical_headers(headers);
    let canonical = format!("{method}\n{uri}\n\n{headers}\n{signed}\n{payload_hash}");
    (canonical, signed)
}

/// Firma SigV4 de un `string_to_sign` ya construido.
#[cfg(feature = "aws")]
pub(crate) fn sigv4_signature(
    secret: &str,
    date: &str,
    region: &str,
    service: &str,
    string_to_sign: &str,
) -> String {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    hmac_sha256_hex(&k_signing, string_to_sign.as_bytes())
}

/// `string_to_sign` de SigV4.
#[cfg(feature = "aws")]
pub(crate) fn sigv4_string_to_sign(amz_date: &str, scope: &str, canonical: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        sha256_hex(canonical.as_bytes())
    )
}

/// Clave de firma de GCS V4 (`GOOG4-HMAC-SHA256`).
#[cfg(feature = "gcp")]
pub(crate) fn gcs_signing_key(secret: &str, date: &str) -> [u8; 32] {
    let k_date = hmac_sha256(b"GOOG4", secret.as_bytes());
    let k_date = hmac_sha256(&k_date, date.as_bytes());
    let k_region = hmac_sha256(&k_date, b"auto");
    let k_service = hmac_sha256(&k_region, b"storage");
    hmac_sha256(&k_service, b"goog4_request")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vector publicado por AWS para un `PUT` firmado con SigV4.
    #[test]
    fn reproduce_el_vector_de_sigv4_de_aws() {
        let payload_hash = "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072";
        let headers = vec![
            (
                "host".to_owned(),
                "examplebucket.s3.amazonaws.com".to_owned(),
            ),
            (
                "date".to_owned(),
                "Fri, 24 May 2013 00:00:00 GMT".to_owned(),
            ),
            ("x-amz-content-sha256".to_owned(), payload_hash.to_owned()),
            ("x-amz-date".to_owned(), "20130524T000000Z".to_owned()),
            (
                "x-amz-storage-class".to_owned(),
                "REDUCED_REDUNDANCY".to_owned(),
            ),
        ];
        let (canonical, signed_headers) =
            canonical_request("PUT", "/test%24file.text", &headers, payload_hash);
        assert_eq!(
            canonical,
            "\
PUT
/test%24file.text

date:Fri, 24 May 2013 00:00:00 GMT
host:examplebucket.s3.amazonaws.com
x-amz-content-sha256:44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072
x-amz-date:20130524T000000Z
x-amz-storage-class:REDUCED_REDUNDANCY

date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class
44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        assert_eq!(
            signed_headers,
            "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class"
        );
        let scope = "20130524/us-east-1/s3/aws4_request";
        let string_to_sign = sigv4_string_to_sign("20130524T000000Z", scope, &canonical);
        let signature = sigv4_signature(
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "20130524",
            "us-east-1",
            "s3",
            &string_to_sign,
        );
        assert_eq!(
            signature,
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        );
    }

    #[test]
    fn el_cuerpo_vacio_tiene_un_hash_estable() {
        assert_eq!(
            sha256_empty_hex(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
