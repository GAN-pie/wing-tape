# Mémo technique — SIGILL (« Illegal instruction ») sur LiteRT / Raspberry Pi 3

**Projet :** wing-tape
**Plateforme cible :** Raspberry Pi 3 (Cortex-A53, ARMv8.0-A baseline), Raspberry Pi OS 64 bits (aarch64)
**Composant concerné :** crate Rust `google-ai-edge-litert` v0.2.0 (bindings CompiledModel) + SDK C++ LiteRT précompilé
**Date :** 12 novembre 2026
**Priorité :** blocant — le programme ne peut pas démarrer sur RPi3

---

## 1. Symptômes observés

1. Compilation de `cargo build` entièrement sans erreur (crate + liaison du SDK précompilé).
2. À l'exécution (`cargo run`), crash immédiat : `Illegal instruction` (SIGILL), avant même l'entrée dans `main`.
3. Premier essai sous gdb : échec de chargement de `libLiteRt.so` (résolu en ajoutant le répertoire des `.so` extraits par le build script au `LD_LIBRARY_PATH`).

## 2. Diagnostic confirmé sous gdb

Arrêt sur SIGILL avec :

```
=> 0x7ff7a13484:        ldaprb  w8, [x8]
0x0000007ff7a13484 in ?? () from .../out/litert_cc_sdk_extracted/litert_cc_sdk/libLiteRt.so
#2  0x0000007ff7fc2a6c in ?? () from /lib/ld-linux-aarch64.so.1
```

Analyse :

- **`ldaprb`** est une instruction load-acquire RCpc, introduite en ARMv8.2 (généralisée en ARMv8.3). Elle n'existe **pas** sur le Cortex-A53 du RPi3, qui est ARMv8.0-A baseline.
- La backtrace (cadres dans `ld-linux-aarch64.so.1`) montre que le crash survient **pendant l'initialisation de `libLiteRt.so` par le loader dynamique** (constructeur ELF / relocations), avant tout appel de notre code.
- Conclusion : le `libLiteRt.so` du SDK binaire précompilé (téléchargé automatiquement par le `build.rs` de la crate, SDK stable 2.2.x) est compilé pour des CPU ARM plus récents que le RPi3. La compilation Rust ne détecte rien car le linker n'exécute jamais les instructions — seul le CPU les exécute à l'exécution.
- Ce problème est connu sur les binaires précompilés LiteRT : un ticket équivalent existe sur `google-ai-edge/LiteRT-LM` (issue #1847, « illegal hardware instruction » sur Raspberry Pi 4, Cortex-A72 également ARMv8.0).

## 3. Ce que le correctif doit couvrir

- Produire un **SDK C++ LiteRT reconstruit depuis les sources, visant ARMv8-A baseline** (`-march=armv8-a`), compatible Cortex-A53.
- Modifier le **build de la crate** pour utiliser ce SDK local au lieu du zip précompilé téléchargé.
- Vérifier l'absence d'instructions hors baseline dans tous les `.so` livrés (`libLiteRt.so` inclus).

## 4. Plan d'intervention proposé

### Étape 1 — Recompiler le SDK C++ LiteRT

Deux options (au choix) :

- **Build natif sur le RPi3** : cloner `google-ai-edge/LiteRT` et suivre le build CMake (`c/cmake` du dépôt + doc « Build LiteRT with CMake » sur le site Google AI Edge). Un build natif clang/gcc visant l'hôte reste en baseline ARMv8-A. Lent sur un A53 (prévoir plusieurs heures), mais sans risque de mauvais `-march`.
- **Cross-compilation depuis x86-64** : utiliser `docker_build/build_with_docker.sh` du dépôt ou le build CMake croisé ; plus rapide, mais **forcer `-march=armv8-a`** (ne pas laisser la toolchain viser ARMv8.2+ par défaut). Les configs Bazel `--config=elinux_aarch64` de la doc « Build LiteRT for ARM boards » visent également le baseline.

Vérification obligatoire avant d'aller plus loin :

```bash
objdump -d libLiteRt.so | grep -E "ldapr|ldadd|casal|swpl|sdot" && echo "INSTRUCTIONS NON-BASELINE DÉTECTÉES"
```

Aucune correspondance = OK. Appliquer le même contrôle aux autres `.so` éventuels (accélérateurs, dispatch).

### Étape 2 — Pointer la crate Rust vers le SDK local

Le `build.rs` de la crate (`build/build.rs`, cf. `LITERT_CC_SDK_DOWNLOAD_URL` / `litert_sdk_version = "2.2.0"`) télécharge `litert_cc_sdk.zip` depuis une URL codée en dur et l'extrait dans `OUT_DIR`. Procédure :

1. `cargo vendor vendor/` puis éditer `vendor/google-ai-edge-litert-0.2.0/build/build.rs` pour :
   - supprimer/contourner le téléchargement, et
   - utiliser les headers + `libLiteRt.so` reconstruits localement (copie dans `OUT_DIR` ou chemin paramétrable via variable d'environnement si le build script le permet — à vérifier dans le fichier).
2. Ajouter l'entrée `[patch]` / configuration vendored correspondante dans `Cargo.toml`.
3. Conserver le `LD_LIBRARY_PATH` actuel ou figer le chemin avec un rpath :

```toml
# .cargo/config.toml
[target.aarch64-unknown-linux-gnu]
rustflags = ["-C", "link-arg=-Wl,-rpath,/chemin/vers/sdk/reconstruit"]
```

### Étape 3 — Valider sur cible

1. `cargo build` puis exécution : plus de SIGILL au démarrage.
2. Sous gdb, confirmer que `main` est atteint et que l'inférence fonctionne avec le modèle utilisé par wing-tape.
3. Contrôle final : `objdump -d` sur le binaire final et les `.so` embarqués.

## 5. Points de vigilance

- **Le modèle aussi** : si le modèle `.tflite`/`.litert` a été compilé avec des optimisations kernel (XNNPACK, etc.) visant des extensions absentes de l'A53 (dotprod, i8mm, fp16), le crash réapparaîtra à l'exécution, cette fois dans les kernels. Le cas échéant, reconvertir/recompiler le modèle en visant baseline ARMv8-A.
- **Performances** : un Cortex-A53 4×1,2 GHz avec 1 Go de RAM sera lent. Jouable pour un modèle modeste ; hors de portée pour un LLM. À prendre en compte dans le dimensionnement.
- **Suivi amont** : ouvrir/compléter une issue côté `google-ai-edge/LiteRT` pour signaler que les binaires précompilés Linux arm64 (SDK 2.2.x) ne sont pas compatibles ARMv8.0 ; un flag baseline de la part de Google réglerait le problème à la source.

## 6. Références

- Ticket similaire (binaires précompilés LiteRT-LM sur RPi4) : https://github.com/google-ai-edge/LiteRT-LM/issues/1847
- Build LiteRT pour cartes ARM (Bazel, configs elinux_aarch64) : https://developers.google.com/edge/litert/build/arm
- Intégration du SDK C++ précompilé (où se procurer / construire les artifacts) : https://developers.google.com/edge/litert/next/cpp_sdk
- Dépôt principal : https://github.com/google-ai-edge/LiteRT
