import { LOADER_V3_PROGRAM_ADDRESS } from '@solana-program/loader-v3';
import {
    createDecoderThatConsumesEntireByteArray,
    downgradeRoleToNonSigner,
    downgradeRoleToReadonly,
    getAccountMetasFromCompiledTransactionMessage,
    getCompiledTransactionMessageDecoder,
    getTransactionDecoder,
    type AccountMeta,
    type CompiledTransactionMessage,
    type ReadonlyUint8Array,
} from '@solana/kit';

type MessageAccountsResolverScope = Readonly<{
    args: Readonly<{ message: ReadonlyUint8Array }>;
}>;

type TransactionAccountsResolverScope = Readonly<{
    args: Readonly<{ transaction: ReadonlyUint8Array }>;
}>;

const compiledMessageDecoder = createDecoderThatConsumesEntireByteArray(getCompiledTransactionMessageDecoder());
const transactionDecoder = createDecoderThatConsumesEntireByteArray(getTransactionDecoder());

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
 * Resolves the remaining `Submit` accounts from the wrapped transaction's static account list.
 * Account order and writable privileges match the wrapped message, while signer privileges are
 * removed because the wrapped signers are not signers of the outer transaction.
 *
 * Mirrors `signer/client/src/instruction.rs`.
 */
export const resolveTransactionAccounts = (scope: TransactionAccountsResolverScope): AccountMeta[] => {
    const messageBytes = transactionDecoder.decode(scope.args.transaction).messageBytes;
    const message = compiledMessageDecoder.decode(messageBytes);
    return getStaticAccountMetas(message).map(account => ({
        ...account,
        // Wrapped signatures authorize the wrapped transaction, not the outer transaction that submits it.
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
