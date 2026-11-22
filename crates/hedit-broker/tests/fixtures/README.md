`tls-cert.der` and `tls-key.der` are a synthetic self-signed test identity for
`example.test`, used only by loopback TLS regression tests. The private key is
public test data. Production uses the OS trust store and port 443; the fixture is
never added to production trust or packaged as a credential.
