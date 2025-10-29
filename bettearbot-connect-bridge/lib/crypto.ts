import { createPublicKey, verify } from 'crypto';
import base58 from 'bs58';

export function verifyEd25519Signature(
  publicKeyStr: string,
  message: string,
  signatureStr: string
): boolean {
  try {
    if (!publicKeyStr.startsWith('ed25519:')) {
      // Bot key is guaranteed to be ed25519 because it's in .env
      throw new Error('Invalid public key format');
    }
    const publicKeyBase58 = publicKeyStr.slice(8);
    const publicKeyBytes = base58.decode(publicKeyBase58);

    if (!signatureStr.startsWith('ed25519:')) {
      throw new Error('Invalid signature format');
    }
    const signatureBase58 = signatureStr.slice(8);
    const signatureBytes = base58.decode(signatureBase58);

    const publicKey = createPublicKey({
      key: Buffer.concat([
        Buffer.from([0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00]),
        publicKeyBytes,
      ]),
      format: 'der',
      type: 'spki',
    });

    return verify(null, Buffer.from(message), publicKey, signatureBytes);
  } catch (error) {
    console.error('Signature verification error:', error);
    return false;
  }
}
