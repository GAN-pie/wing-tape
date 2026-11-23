# Cross-compilation de LiteRT pour Raspberry Pi 3B (aarch64)

Guide de reproduction de la cross-compilation de LiteRT depuis un hôte x86-64 GNU/Linux
vers une cible Raspberry Pi 3 B (Raspberry Pi OS 64-bit, glibc 2.41).

Branche concernée : `feats/rpi3-crosscompilation`

---

## 1. Vue d'ensemble

| Élément | Valeur |
|---|---|
| Hôte de build | x86-64 GNU/Linux (Debian/Ubuntu) |
| Cible | Raspberry Pi 3 B, Raspberry Pi OS 64-bit (bookworm) |
| CPU cible | Cortex-A53 (ARMv8-A, aarch64) |
| glibc cible | 2.41 |
| Toolchain | `gcc-aarch64-linux-gnu` / `g++-aarch64-linux-gnu` (GCC ≥ 15 testé) |
| Sysroot | Copie rsync du système de fichiers du Pi (IP `192.168.1.19`) |
| Artefact final | `libLiteRT.so` consommé par le runtime Rust (`litert/rust`) |

Principe clé : le compilateur croisé de la distribution embarque sa **propre**
glibc (souvent plus récente que celle de la cible). Sans discipline de sysroot,
les binaires finissent liés contre la glibc de l'hôte et refusent de démarrer
sur le Pi (`version GLIBC_x.xx not found`). Tout le dispositif ci-dessous vise
à garantir que **chaque** bibliothèque résolue à l'édition de liens provient du
sysroot du Pi.

---

## 2. Prérequis sur l'hôte

```bash
sudo apt update
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu rsync
```

Compilateurs installés : `/usr/bin/aarch64-linux-gnu-gcc{,++}`.
Pas besoin de symlink ni de toolchain tiers (le toolchain file résout le
préfixe `aarch64-linux-gnu-` automatiquement).

---

## 3. Construction du sysroot

### 3.1 Synchronisation depuis le Pi (rsync, sans sudo)

`/usr/include` et `/usr/lib` sont lisibles par tous sur Raspberry Pi OS :

```bash
mkdir -p ~/sysroots/rpi3/usr/lib

rsync -azv mandrake@192.168.1.19:/usr/include ~/sysroots/rpi3/usr/
rsync -azv mandrake@192.168.1.19:/usr/lib/aarch64-linux-gnu/ \
  ~/sysroots/rpi3/usr/lib/aarch64-linux-gnu/
rsync -azv mandrake@192.168.1.19:/usr/lib/pkgconfig ~/sysroots/rpi3/usr/lib/
```

Ne synchronise **pas** `/lib` entier avec `-L` : sur Raspberry Pi OS 64-bit
(merged-/usr), `/lib` est un symlink vers `usr/lib` et une recopie déréférencée
tire des centaines de Mo inutiles (firmware, Perl, Python…). Seul le répertoire
multiarch `usr/lib/aarch64-linux-gnu` est nécessaire pour lier.

> Le user `mandrake` est un placeholder : adapter à l'utilisateur du Pi.
> Resynchroniser après chaque montée de version de l'OS du Pi (rsync incrémental).

### 3.2 Corrections de layout (indispensables)

Le toolchain croisé GCC s'attend à certains chemins ; le layout Debian
multiarch + merged-/usr impose trois ajustements :

```bash
# 1) /lib à la racine du sysroot -> usr/lib
#    (les scripts linker du Pi référencent /lib/aarch64-linux-gnu/...)
ln -sfn usr/lib ~/sysroots/rpi3/lib

# 2) symlink du loader dynamique là où le script linker l'attend
ln -s aarch64-linux-gnu/ld-linux-aarch64.so.1 \
  ~/sysroots/rpi3/lib/ld-linux-aarch64.so.1
```

Vérifications :

```bash
ls ~/sysroots/rpi3/usr/include/stdio.h
ls -lL ~/sysroots/rpi3/lib/aarch64-linux-gnu/libm.so.6
ls -lL ~/sysroots/rpi3/lib/ld-linux-aarch64.so.1
```

(`ls -lL` suit les liens : toute réponse « Aucun fichier » = symlink cassé.)

### 3.3 Snapshot de référence

Une fois le build validé, figer le sysroot connu-bon :

```bash
tar -czf ~/sysroots/rpi3-sysroot-glibc2.41.tgz -C ~/sysroots rpi3
```

Ce tarball est la baseline reproductible, indépendante de l'état du Pi.

---

## 4. Toolchain file : `vendors/rpi3/cmake/toolchains.cmake`

Fichier dédié (remplace l'exemple Broadcom `vendors/broadcom/cmake/toolchains.cmake`,
qui exigeait son propre layout `toolchain=`) :

```cmake
# Cross-compilation Raspberry Pi 3B (aarch64, Raspberry Pi OS 64-bit)
# depuis hôte x86-64 GNU/Linux. Preset : "linux-arm64".
# Overrides : -DRPI3_SYSROOT=...  -DCROSS_PREFIX=aarch64-linux-gnu-

if(RPI3_TOOLCHAIN_INCLUDED)
  return()
endif()
set(RPI3_TOOLCHAIN_INCLUDED TRUE)

set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

if(NOT DEFINED RPI3_SYSROOT)
  set(RPI3_SYSROOT "$ENV{HOME}/sysroots/rpi3" CACHE PATH "Raspberry Pi 3 sysroot")
endif()

if(NOT EXISTS "${RPI3_SYSROOT}/usr/include")
  message(FATAL_ERROR "RPI3_SYSROOT '${RPI3_SYSROOT}' has no usr/include - sync the Pi sysroot first with rsync")
endif()

set(CMAKE_SYSROOT "${RPI3_SYSROOT}")

if(NOT DEFINED CROSS_PREFIX)
  set(CROSS_PREFIX "aarch64-linux-gnu-" CACHE STRING "Cross-compiler prefix")
endif()

find_program(RPI3_CC ${CROSS_PREFIX}gcc REQUIRED)
find_program(RPI3_CXX ${CROSS_PREFIX}g++ REQUIRED)

set(CMAKE_C_COMPILER "${RPI3_CC}")
set(CMAKE_CXX_COMPILER "${RPI3_CXX}")

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)

# Le -L doit PRECEDER les répertoires internes de gcc-cross :
# sans lui, ld prend les scripts linker de l'hôte (glibc la plus récente).
set(CMAKE_EXE_LINKER_FLAGS_INIT "-L${RPI3_SYSROOT}/usr/lib/aarch64-linux-gnu")
set(CMAKE_SHARED_LINKER_FLAGS_INIT "-L${RPI3_SYSROOT}/usr/lib/aarch64-linux-gnu")
set(CMAKE_MODULE_LINKER_FLAGS_INIT "-L${RPI3_SYSROOT}/usr/lib/aarch64-linux-gnu")

list(APPEND CMAKE_TRY_COMPILE_PLATFORM_VARIABLES RPI3_SYSROOT CROSS_PREFIX)
```

Points de conception :

- **`-L` en premier** : ld explore les répertoires `-L` avant ses répertoires
  par défaut. C'est ce qui fait gagner les scripts linker **du Pi**
  (`usr/lib/aarch64-linux-gnu/libc.so`, `libm.so`, scripts `GROUP(...)`)
  face à ceux de `gcc-cross` (qui pointent vers la glibc de l'hôte).
  Leurs chemins absolus (`/lib/aarch64-linux-gnu/...`, `/usr/lib/...`) sont
  alors préfixés par `--sysroot` et existent dans le sysroot grâce au § 3.2.
- Pas de `--sysroot` manuel dans les flags : `CMAKE_SYSROOT` l'injecte
  automatiquement en compilation et en édition de liens.
- `CMAKE_FIND_ROOT_PATH_MODE_*` : cloisonne `find_path`/`find_library`
  dans le sysroot, aucune fuite possible depuis l'hôte.
- Garde anti-réinclusion + export vers `try_compile` (mêmes patterns que le
  toolchain Broadcom de référence).

---

## 5. Preset CMake : `linux-arm64`

Dans `CMakePresets.json` (extrait modifié) :

```json
{
  "name": "linux-arm64",
  "displayName": "Linux arm64-v8a Debug",
  "generator": "Unix Makefiles",
  "binaryDir": "${sourceDir}/cmake_build_arm64",
  "toolchainFile": "${sourceDir}/vendors/rpi3/cmake/toolchains.cmake",
  "cacheVariables": {
    "CMAKE_SYSTEM_NAME": "Linux",
    "CMAKE_SYSTEM_PROCESSOR": "aarch64",
    "CMAKE_BUILD_TYPE": "Debug",
    "TFLITE_ENABLE_GPU": "OFF",
    "TFLITE_ENABLE_XNNPACK": "ON",
    "LITERT_ENABLE_GPU": "OFF",
    "LITERT_DISABLE_KLEIDIAI": "ON",
    "LITERT_ENABLE_ALL_VENDORS": "OFF",
    "LITERT_AUTO_BUILD_TFLITE": "ON",
    "LITERT_HOST_C_COMPILER": "/usr/bin/gcc",
    "LITERT_HOST_CXX_COMPILER": "/usr/bin/g++",
    "CMAKE_C_FLAGS": "-mcpu=cortex-a53 -funsafe-math-optimizations",
    "CMAKE_CXX_FLAGS": "-mcpu=cortex-a53 -funsafe-math-optimizations",
    "CMAKE_POLICY_VERSION_MINIMUM": "3.5"
  }
}
```

Notes :

- **`-mcpu=cortex-a53`** est le seul flag d'architecture nécessaire en
  aarch64. Les options `-mfpu`, `-mfloat-abi`, `-mfp16-format` sont des
  options du mode 32 bits ARM — ne pas les utiliser en 64 bits. Le Cortex-A53
  ne supporte pas `armv8.2-a+fp16`, d'où les warnings XNNPACK attendus à
  l'exécution : les ops fp16 sont retombées en fp32 (voir § 8).
- `-funsafe-math-optimizations` : accélère le FP mais peut modifier
  légèrement les résultats numériques (acceptable pour l'inférence).
- **`LITERT_HOST_C/CXX_COMPILER` = `/usr/bin/gcc`** : NE PAS supprimer.
  « host » désigne ici la machine de build : LiteRT génère des outils
  hébergés pendant la compilation (générateurs de code, parsing…), qui
  doivent être compilés en x86-64 natif pendant que le reste croise vers
  aarch64.
- `CMAKE_POLICY_VERSION_MINIMUM=3.5` : compatibilité avec les sous-projets
  anciens tirés par FetchContent.

---

## 6. Build

```bash
cmake --preset "linux-arm64"
cmake --build cmake_build_arm64 -j$(nproc)
```

En cas de modification du toolchain file ou des flags linker : **supprimer
`cmake_build_arm64/`** avant de reconfigurer — les résultats de probing
compilateur et les `_INIT` flags sont gravés dans le cache au premier configure.

---

## 7. Vérification de la compatibilité glibc

Après build, contrôler le plafond de versions de symboles exigé :

```bash
objdump -T cmake_build_arm64/tools/run_model \
  | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -3
```

Résultat attendu avec ce setup : plafond à `GLIBC_2.38` (<= 2.41, OK).
Tout tag `>= 2.42` signifie qu'une bibliothèque a été résolue hors sysroot.

Diagnostic si nécessaire :

```bash
# fichier de test minimal utilisé comme cobaye pour la compilation croisée
# (recréer au besoin : echo 'int main(){return 0;}' > /tmp/t.c)
echo 'int main(){return 0;}' > /tmp/t.c

# quels symboles exigent la version fautive ?
objdump -T <binaire> | grep GLIBC_2.43

# quelle bibliothèque ld résout-il réellement ?
# /tmp/t.c = code de test minimal (voir ci-dessus) ; on force --sysroot
# volontairement pour observer la résolution ld en dehors de CMake.
aarch64-linux-gnu-g++ --sysroot=$HOME/sysroots/rpi3 /tmp/t.c \
  -o /tmp/t -lm -Wl,-t 2>&1 | grep -E 'libm|libc'
# => tous les chemins doivent être sous $HOME/sysroots/rpi3/
```

Symptômes connus et leur cause :

| Erreur ld | Cause | Correction |
|---|---|---|
| `ne peut trouver /lib/aarch64-linux-gnu/libm.so.6 à l'intérieur du sysroot` | `/lib` absent du sysroot | `ln -sfn usr/lib $SYSROOT/lib` |
| `ne peut trouver /lib/ld-linux-aarch64.so.1` | symlink du loader absent | `ln -s aarch64-linux-gnu/ld-linux-aarch64.so.1 $SYSROOT/lib/` |
| Tags `GLIBC_x.y` trop récents | scripts linker de l'hôte gagnent la course de résolution | flag `-L$SYSROOT/usr/lib/aarch64-linux-gnu` (§ 4) |
| Tag `GLIBC_2.43` sur `atan2f`/`sqrtf` | `-lm` résolu depuis l'hôte (fonctions re-versionnées en 2.43) | idem ci-dessus |

---

## 8. Intégration au runtime Rust

Le script `litert/rust/build/build.rs` télécharge par défaut un runtime
pré-compilé depuis les serveurs Google. Pour consommer notre `libLiteRT.so`
croisé :

1. **Emplacement attendu** : le script cherche le .so à la route
   `binaries/2.3.0/linux_arm64/libLiteRt.so` relative à la racine du repo
   LiteRT. Placer l'artefact du build croisé à :

   ```
   LiteRT/litert/binaries/2.3.0/linux_arm64/libLiteRt.so
   ```

   (nom du fichier : `libLiteRt.so`, casing exact, dans le dossier
   `linux_arm64` — underscore, pas tiret).

2. **Serveur HTTP local** : le démarrer depuis `LiteRT/litert/` (et non la
   racine du repo) afin que l'URL servie colle à la route attendue :

   ```bash
   cp cmake_build_arm64/<chemin-du-build>/libLiteRt.so \
      binaries/2.3.0/linux_arm64/libLiteRt.so
   cd LiteRT/litert
   python3 -m http.server 8080
   # => http://localhost:8080/binaries/2.3.0/linux_arm64/libLiteRt.so
   ```

3. **build.rs** : bypasser le téléchargement officiel et pointer l'URL vers
   `http://localhost:8080/binaries/2.3.0/linux_arm64/libLiteRt.so`
   (modification incluse sur la branche `feats/rpi3-crosscompilation`).

À l'exécution sur le Pi, les warnings XNNPACK suivants sont **attendus et
normaux** : le Cortex-A53 ne supporte pas `armv8.2-a+fp16`, donc les ops
fp16 du modèle sont retombées en fp32 :

```
XNNPACK: ... falling back to fp32 ...
```

---

## 9. Checklist de reproduction (résumé opératoire)

```bash
# 0. Prérequis
sudo apt install gcc-aarch64-linux-gnu g++-aarch64-linux-gnu rsync

# 1. Sysroot
mkdir -p ~/sysroots/rpi3/usr/lib
rsync -azv <user>@192.168.1.19:/usr/include ~/sysroots/rpi3/usr/
rsync -azv <user>@192.168.1.19:/usr/lib/aarch64-linux-gnu/ \
  ~/sysroots/rpi3/usr/lib/aarch64-linux-gnu/
rsync -azv <user>@192.168.1.19:/usr/lib/pkgconfig ~/sysroots/rpi3/usr/lib/
ln -sfn usr/lib ~/sysroots/rpi3/lib
ln -s aarch64-linux-gnu/ld-linux-aarch64.so.1 ~/sysroots/rpi3/lib/ld-linux-aarch64.so.1

# 2. Vérifier le sysroot
ls ~/sysroots/rpi3/usr/include/stdio.h
ls -lL ~/sysroots/rpi3/lib/aarch64-linux-gnu/libm.so.6
ls -lL ~/sysroots/rpi3/lib/ld-linux-aarch64.so.1

# 3. Build (preset + toolchain file fournis dans le repo)
rm -rf cmake_build_arm64          # cache propre
cmake --preset "linux-arm64"
cmake --build cmake_build_arm64 -j$(nproc)

# 4. Contrôle glibc (plafond attendu : GLIBC_2.38)
objdump -T cmake_build_arm64/tools/run_model \
  | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -3

# 5. Test end-to-end sur le Pi
scp cmake_build_arm64/tools/run_model <user>@192.168.1.19:~/
ssh <user>@192.168.1.19 './run_model ...'

# 6. Runtime Rust
#    - copier libLiteRt.so vers binaries/2.3.0/linux_arm64/libLiteRt.so
#      (relatif à LiteRT/litert/)
#    - démarrer `python3 -m http.server 8080` depuis LiteRT/litert/
#    - build.rs (branche feats/rpi3-crosscompilation) pointe sur
#      http://localhost:8080/binaries/2.3.0/linux_arm64/libLiteRt.so

# 7. Figer le sysroot validé
tar -czf ~/sysroots/rpi3-sysroot-glibc2.41.tgz -C ~/sysroots rpi3
```

---

## 10. Pièges rencontrés (mémo)

1. **`sudo rsync` distant impossible** : sudo ne peut pas demander de mot de
   passe sur un canal rsync non interactif. Inutile ici : `/usr/include` et
   `/usr/lib` sont world-readable.
2. **Recopie de `/lib` avec `-L`** : très coûteuse et inutile (merged-/usr) ;
   seul `usr/lib/aarch64-linux-gnu` compte pour le linkage.
3. **Scripts linker Debian** : `libc.so`/`libm.so` sont des scripts `GROUP(...)`
   avec chemins absolus. Ils ne sont corrects que si (a) ld reçoit `-L` vers le
   multiarch du sysroot **avant** ses défauts, et (b) les chemins `/lib/...`
   existent dans le sysroot (symlink § 3.2).
4. **Cache CMake** : tout changement de toolchain/flags `_INIT` exige une
   suppression du build dir.
5. **`LITERT_HOST_*_COMPILER`** : compilent les outils *hébergés* du build ;
   ne pas les supprimer du preset.
6. **Alternative container (Docker arm64 + binfmt)** : reproductible mais
   5-15× plus lent (émulation QEMU) ; le pattern « container comme source de
   sysroot » reste viable pour la CI. La synchronisation depuis le Pi réel
   garantit en prime l'exactitude des libs spécifiques Raspberry Pi OS.
