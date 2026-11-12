// Node'un crypto modulunun tarayicida gereken kismi: yalniz rastgele sayi.
export const webcrypto = globalThis.crypto;
export function getRandomValues(a) {
  return globalThis.crypto.getRandomValues(a);
}
export function randomBytes(n) {
  return Buffer.from(globalThis.crypto.getRandomValues(new Uint8Array(n)));
}
export default { webcrypto, getRandomValues, randomBytes };
