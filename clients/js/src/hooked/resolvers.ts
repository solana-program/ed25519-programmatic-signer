import { LOADER_V3_PROGRAM_ADDRESS } from '@solana-program/loader-v3';
import {
    createDecoderThatConsumesEntireByteArray,
    downgradeRoleToNonSigner,
    downgradeRoleToReadonly,
    getAccountMetasFromCompiledTransactionMessage,
    getCompiledTransactionMessageDecoder,
    type AccountMeta,
    type CompiledTransactionMessage,
    type ReadonlyUint8Array,
} from '@solana/kit';

type MessageAccountsResolverScope = Readonly<{
    args: Readonly<{ message: ReadonlyUint8Array }>;
}>;

const compiledMessageDecoder = createDecoderThatConsumesEntireByteArray(getCompiledTransactionMessageDecoder());

/**
 * Resolves the remaining `Execute` accounts from the wrapped message's static account list.
 * Accounts keep the order and permissions they would have in a normal Solana transaction.
 *
 * Mirrors `executor/client/src/instruction.rs`.
 */
export const resolveMessageAccounts = (scope: MessageAccountsResolverScope): AccountMeta[] => {
    const message = compiledMessageDecoder.decode(scope.args.message);
    return getStaticAccountMetas(message);
};

/**
 * Resolves the remaining `Submit` accounts from the wrapped message's static account list.
 * Account order and writable privileges match the wrapped message, while signer privileges are
 * removed because the wrapped signers are not signers of the outer transaction.
 *
 * Mirrors `signer/client/src/instruction.rs`.
 */
export const resolveSubmitMessageAccounts = (scope: MessageAccountsResolverScope): AccountMeta[] => {
    return resolveMessageAccounts(scope).map(account => ({
        ...account,
        // Wrapped signatures authorize the wrapped message, not the outer transaction that submits it.
        role: downgradeRoleToNonSigner(account.role),
    }));
};

// Builds account metas out of a message's static account list.
const getStaticAccountMetas = (message: CompiledTransactionMessage): AccountMeta[] => {
    const programAccountIndexes = getProgramAccountIndexes(message);

    // Program accounts are normally readonly, but must remain writable when the upgradeable loader may upgrade one.
    const hasUpgradeableLoader = message.staticAccounts.includes(LOADER_V3_PROGRAM_ADDRESS);

    return getAccountMetasFromCompiledTransactionMessage(message).map((account, index) => {
        const isDemotedProgram = programAccountIndexes.has(index) && !hasUpgradeableLoader;
        return {
            ...account,
            role: isDemotedProgram ? downgradeRoleToReadonly(account.role) : account.role,
        };
    });
};

const getProgramAccountIndexes = (message: CompiledTransactionMessage): Set<number> => {
    return new Set(
        message.version === 1
            ? message.instructionHeaders.map(instruction => instruction.programAccountIndex)
            : message.instructions.map(instruction => instruction.programAddressIndex),
    );
};
