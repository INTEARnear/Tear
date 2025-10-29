import { createHash } from 'crypto';
import { Schema, serialize } from 'borsh';
import { PublicKey } from '@near-js/crypto';
import { SignMessageParams } from '@hot-labs/near-connect';

export class Payload {
  tag: number;
  message: string;
  nonce: Buffer;
  recipient: string;
  callbackUrl?: string;

  constructor(data: SignMessageParams) {
    this.tag = 2 ** 31 + 413;
    this.message = data.message;
    this.nonce = Buffer.from(data.nonce);
    this.recipient = data.recipient;
  }
}

export const payloadSchema: Schema = {
  struct: {
    tag: 'u32',
    message: 'string',
    nonce: { array: { type: 'u8', len: 32 } },
    recipient: 'string',
    callbackUrl: { option: 'string' },
  },
};

export interface VerifySignatureParams {
  publicKey: string;
  signature: string;
  message: string;
  nonce: Uint8Array;
  recipient: string;
}

export function verifySignature({
  publicKey,
  signature,
  message,
  nonce,
  recipient,
}: VerifySignatureParams): boolean {
  try {
    const payload = new Payload({
      message,
      nonce,
      recipient,
    });
    const borshPayload = serialize(payloadSchema, payload);
    const hashedPayload = createHash('sha256').update(borshPayload).digest();
    const providedSignature = Buffer.from(signature, 'base64');
    return PublicKey.from(publicKey).verify(hashedPayload, providedSignature);
  } catch (error) {
    console.error('NEP-413 signature verification error:', error);
    return false;
  }
}
