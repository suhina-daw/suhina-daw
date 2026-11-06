// SPDX-License-Identifier: Apache-2.0

#[test]
#[ignore = "requires SUHINA_ASIO_TEST_DRIVER naming an installed, connected driver"]
fn freshly_loaded_driver_exposes_output_format() {
    let name = std::env::var("SUHINA_ASIO_TEST_DRIVER").expect("test driver required");
    let asio = super::Asio::new();
    for _ in 0..3 {
        let driver = asio.load_driver(&name).expect("load test driver");
        // Match CPAL stream creation: query the format immediately after loading,
        // without retaining the driver used during device enumeration.
        assert!(driver.output_data_type().is_ok());
        assert!(driver.destroy().expect("release test driver"));
    }
}
