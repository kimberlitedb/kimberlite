(* ========================================================================= *)
(* Coq Extraction Configuration for Rust                                    *)
(*                                                                           *)
(* This module configures extraction of verified Coq specifications to      *)
(* OCaml, which is then wrapped in Rust. The extraction produces:           *)
(*   - Type definitions (bytes, keys, signatures)                           *)
(*   - Function signatures (abstract specifications)                        *)
(*   - Proof certificates (embedded in Rust wrappers)                       *)
(*                                                                           *)
(* Extraction Strategy:                                                      *)
(*   1. Extract to OCaml (Coq's native extraction target)                   *)
(*   2. Parse OCaml output to generate Rust trait definitions                *)
(*   3. Implement traits using existing Rust crypto libraries               *)
(*   4. Embed proof certificates in Rust code                               *)
(* ========================================================================= *)

Require Extraction.
Require Import Kimberlite.Common.
Require Import Kimberlite.SHA256.
Require Import Kimberlite.BLAKE3.
Require Import Kimberlite.AES_GCM.
Require Import Kimberlite.Ed25519.
Require Import Kimberlite.KeyHierarchy.

(* ------------------------------------------------------------------------- *)
(* Extraction Language Configuration                                         *)
(* ------------------------------------------------------------------------- *)

(* Extract to OCaml (Coq's best-supported target) *)
Extraction Language OCaml.

(* Use efficient OCaml primitives *)
Set Extraction AccessOpaque.
Set Extraction KeepSingleton.

(* ------------------------------------------------------------------------- *)
(* Type Mappings (Coq → OCaml → Rust)                                       *)
(* ------------------------------------------------------------------------- *)

(* Basic types *)
Extract Inductive bool => "bool" [ "true" "false" ].
Extract Inductive option => "option" [ "Some" "None" ].
Extract Inductive list => "list" [ "[]" "(::)" ].
Extract Inductive prod => "(*)"  [ "(,)" ].

(* Numeric types *)
Require Import Coq.NArith.NArith.
Extract Inductive N => "int" [ "0" "((+) 1)" ]
  "(fun fO fS n -> if n=0 then fO () else fS (n-1))".

(* Natural numbers *)
Extract Inductive nat => "int" [ "0" "succ" ]
  "(fun fO fS n -> if n=0 then fO () else fS (n-1))".

(* ------------------------------------------------------------------------- *)
(* Optimizations                                                              *)
(* ------------------------------------------------------------------------- *)

(* Inline simple definitions *)
Extraction Inline proj1_sig proj2_sig.
Extraction Inline fst snd.

(* Optimize list operations *)
Extraction Inline app length.

(* ------------------------------------------------------------------------- *)
(* Extract Proof Certificates                                                 *)
(* ------------------------------------------------------------------------- *)

(* Proof certificates are extracted as OCaml records, then converted to Rust *)
Extraction ProofCertificate.

(* SHA-256 certificates *)
Extraction sha256_deterministic_certificate.
Extraction sha256_non_degenerate_certificate.
Extraction chain_hash_genesis_integrity_certificate.

(* BLAKE3 certificates *)
Extraction blake3_deterministic_certificate.
Extraction blake3_non_degenerate_certificate.
Extraction blake3_tree_construction_soundness_certificate.

(* AES-GCM certificates *)
Extraction aes_gcm_roundtrip_certificate.
Extraction aes_gcm_integrity_certificate.
Extraction nonce_uniqueness_certificate.
Extraction ind_cca2_certificate.

(* Ed25519 certificates *)
Extraction ed25519_verify_correctness_certificate.
Extraction ed25519_euf_cma_certificate.
Extraction ed25519_determinism_certificate.
Extraction key_derivation_uniqueness_certificate.

(* Key Hierarchy certificates *)
Extraction tenant_isolation_certificate.
Extraction key_wrapping_soundness_certificate.
Extraction forward_secrecy_certificate.
Extraction key_derivation_injective_certificate.

(* ------------------------------------------------------------------------- *)
(* Extract Type Definitions                                                   *)
(* ------------------------------------------------------------------------- *)

(* Basic types *)
Extraction bytes.
Extraction byte.

(* Sized byte arrays (will become Rust [u8; N]) *)
Extraction bytes32.
Extraction bytes64.

(* Cryptographic types *)
Extraction position.
Extraction ChainHash.

(* SHA-256 *)
Extraction sha256_hash.

(* BLAKE3 *)
Extraction blake3_hash.

(* AES-GCM *)
Extraction aes256_key.
Extraction gcm_nonce.
Extraction AuthenticatedCiphertext.

(* Ed25519 *)
Extraction ed25519_secret_key.
Extraction ed25519_public_key.
Extraction ed25519_signature.

(* Key Hierarchy *)
Extraction MasterKey.
Extraction KEK.
Extraction DEK.
Extraction WrappedDEK.
Extraction TenantId.
Extraction StreamId.

(* ------------------------------------------------------------------------- *)
(* Extract Function Specifications                                            *)
(* ------------------------------------------------------------------------- *)

(* SHA-256 *)
Extraction sha256_bytes.
Extraction chain_hash.

(* BLAKE3 *)
Extraction blake3_bytes.
Extraction blake3_tree.

(* AES-GCM *)
Extraction aes_gcm_encrypt.
Extraction aes_gcm_decrypt.
Extraction position_to_nonce.

(* Ed25519 *)
Extraction derive_public_key.
Extraction ed25519_sign.
Extraction ed25519_verify.

(* Key Hierarchy *)
Extraction derive_kek.
Extraction derive_dek.
Extraction wrap_dek.
Extraction unwrap_dek.

(* ------------------------------------------------------------------------- *)
(* Separate Extraction (one file per module)                                 *)
(* ------------------------------------------------------------------------- *)

(* Extract to separate OCaml files *)
Separate Extraction
  (* Common *)
  ProofCertificate

  (* SHA-256 *)
  sha256_bytes
  chain_hash
  sha256_deterministic_certificate
  sha256_non_degenerate_certificate
  chain_hash_genesis_integrity_certificate

  (* BLAKE3 *)
  blake3_bytes
  blake3_tree
  blake3_deterministic_certificate
  blake3_non_degenerate_certificate
  blake3_tree_construction_soundness_certificate

  (* AES-GCM *)
  aes_gcm_encrypt
  aes_gcm_decrypt
  position_to_nonce
  aes_gcm_roundtrip_certificate
  aes_gcm_integrity_certificate
  nonce_uniqueness_certificate
  ind_cca2_certificate

  (* Ed25519 *)
  derive_public_key
  ed25519_sign
  ed25519_verify
  ed25519_verify_correctness_certificate
  ed25519_euf_cma_certificate
  ed25519_determinism_certificate
  key_derivation_uniqueness_certificate

  (* Key Hierarchy *)
  derive_kek
  derive_dek
  wrap_dek
  unwrap_dek
  tenant_isolation_certificate
  key_wrapping_soundness_certificate
  forward_secrecy_certificate
  key_derivation_injective_certificate.

(* ------------------------------------------------------------------------- *)
(* Extraction Notes                                                           *)
(* ------------------------------------------------------------------------- *)

(*
   EXTRACTION WORKFLOW:

   1. Run extraction:
      ```bash
      cd specs/coq
      coqc -Q . Kimberlite Extract.v
      ```

   2. Generated files (in specs/coq/):
      - Extract.ml, Extract.mli
      - Contains OCaml types and function signatures

   3. Parse OCaml → Generate Rust traits:
      ```bash
      # Manual step: read Extract.mli, write Rust trait definitions
      # Could be automated with OCaml parser
      ```

   4. Implement traits in Rust:
      - Use existing crypto libraries (sha2, blake3, aes-gcm, ed25519-dalek)
      - Wrap implementations with proof certificates

   RUST INTEGRATION PATTERN:

   ```rust
   // Generated from Coq
   pub trait VerifiedSha256 {
       fn hash(data: &[u8]) -> [u8; 32];
       fn proof_certificate() -> ProofCertificate;
   }

   // Implementation using sha2 crate
   impl VerifiedSha256 for Sha256Impl {
       fn hash(data: &[u8]) -> [u8; 32] {
           // Call sha2::Sha256
       }

       fn proof_certificate() -> ProofCertificate {
           // Embed Coq proof certificate
       }
   }
   ```

   TYPE MAPPING (Coq → OCaml → Rust):

   - bytes               → int list        → Vec<u8>
   - bytes32             → int list        → [u8; 32]
   - option T            → T option        → Option<T>
   - ProofCertificate    → record          → struct ProofCertificate

   AXIOM HANDLING:

   Coq Parameters (axioms) are extracted as OCaml function signatures
   without implementations. These become Rust trait methods that MUST
   be implemented using vetted cryptographic libraries.

   PROOF CERTIFICATE EMBEDDING:

   Each verified function includes its ProofCertificate, enabling:
   - Runtime verification that code matches spec
   - Audit trail of which theorems apply
   - Compliance documentation generation
*)
