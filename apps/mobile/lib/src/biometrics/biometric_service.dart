/// Biometric unlock interface — real device biometrics are out of scope for CI.
abstract class BiometricService {
  Future<bool> get isAvailable;
  Future<bool> authenticate({String reason = 'Unlock CompanyOS'});
}

/// Fake biometric used in CI (no Secure Enclave / Keystore).
class FakeBiometricService implements BiometricService {
  FakeBiometricService({
    this.available = true,
    this.shouldSucceed = true,
  });

  bool available;
  bool shouldSucceed;
  int attemptCount = 0;

  @override
  Future<bool> get isAvailable async => available;

  @override
  Future<bool> authenticate({String reason = 'Unlock CompanyOS'}) async {
    attemptCount += 1;
    return shouldSucceed && available;
  }
}
