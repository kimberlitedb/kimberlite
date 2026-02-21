/**
 * Ambient module declarations for FFI packages that lack TypeScript types.
 * Using shorthand declarations so all imports from these modules are typed as `any`.
 */
declare module 'ffi-napi';
declare module 'ref-napi';
declare module 'ref-struct-napi';
declare module 'ref-array-napi';
