import { AccountRole, createNoopSigner, getAddressDecoder } from '@solana/kit';
import { describe, expect, it } from 'vitest';

import {
    getWithdrawInstruction,
    getWithdrawInstructionDataCodec,
    identifyNonceInstruction,
    NONCE_PROGRAM_ADDRESS,
    NonceInstruction,
    parseWithdrawInstruction,
} from '../src';

describe('withdraw', () => {
    it('matches the Rust wire format', () => {
        const codec = getWithdrawInstructionDataCodec();
        const bytes = codec.encode({ lamports: 0x0807060504030201n });
        expect(Array.from(bytes)).toEqual([2, 1, 2, 3, 4, 5, 6, 7, 8]);
        expect(codec.decode(bytes)).toEqual({ discriminator: 2, lamports: 0x0807060504030201n });
        expect(identifyNonceInstruction(bytes)).toBe(NonceInstruction.Withdraw);
    });

    it('builds the authority, nonce, and destination account roles', () => {
        const decoder = getAddressDecoder();
        const authority = createNoopSigner(decoder.decode(new Uint8Array(32).fill(1)));
        const nonceAccount = decoder.decode(new Uint8Array(32).fill(2));
        const destination = decoder.decode(new Uint8Array(32).fill(3));
        const instruction = getWithdrawInstruction({ authority, nonceAccount, destination, lamports: 100 });
        expect(instruction.programAddress).toBe(NONCE_PROGRAM_ADDRESS);
        expect(instruction.accounts.map(({ address, role }) => ({ address, role }))).toEqual([
            { address: authority.address, role: AccountRole.READONLY_SIGNER },
            { address: nonceAccount, role: AccountRole.WRITABLE },
            { address: destination, role: AccountRole.WRITABLE },
        ]);
        expect(parseWithdrawInstruction(instruction).data).toEqual({ discriminator: 2, lamports: 100n });
    });
});
