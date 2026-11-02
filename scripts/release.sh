#!/usr/bin/env bash
# scripts/release.sh
# Automatiza a criação de nova versão/release:
# 1. Pergunta ou incrementa versão patch no Cargo.toml
# 2. Atualiza Cargo.lock e compila em release
# 3. Gera pacotes locais: DEB, RPM, AppImage e Tarball
# 4. Commita com timestamp, cria tag git e faz push para origin

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Garante que estamos na raiz de um repositório git
if [ ! -d ".git" ]; then
  echo "Erro: Repositório Git não encontrado em $PROJECT_ROOT." >&2
  exit 1
fi

# Detecta versão atual no Cargo.toml
CURRENT_VERSION=$(grep -m1 '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d '"' -f 2)
if [ -z "$CURRENT_VERSION" ]; then
  echo "Erro: Não foi possível detectar a versão em Cargo.toml." >&2
  exit 1
fi

# Calcula próxima versão sugerida (incremento do último dígito)
BASE_PREFIX="${CURRENT_VERSION%.*}"
PATCH_NUM="${CURRENT_VERSION##*.}"
if [[ "$PATCH_NUM" =~ ^[0-9]+$ ]]; then
  SUGGESTED_VERSION="${BASE_PREFIX}.$((PATCH_NUM + 1))"
else
  SUGGESTED_VERSION="${CURRENT_VERSION}.1"
fi

echo "=================================================="
echo "      WhattyTheShell! - Automação de Release      "
echo "=================================================="
echo "Versão atual: $CURRENT_VERSION"
echo "Versão sugerida: $SUGGESTED_VERSION"
echo ""

# Pergunta ao usuário qual versão deseja usar
if [ -t 0 ]; then
  read -r -p "Digite a nova versão ou pressione Enter para [$SUGGESTED_VERSION]: " USER_INPUT
else
  USER_INPUT=""
fi

NEW_VERSION="${USER_INPUT:-$SUGGESTED_VERSION}"

# Validação simples de formato semver
if ! [[ "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Erro: Formato de versão inválido: '$NEW_VERSION'" >&2
  exit 1
fi

echo ""
echo "==> Definindo nova versão: $NEW_VERSION"

# 1. Atualiza Cargo.toml
# Altera apenas a primeira ocorrência de 'version = "..."' que pertence ao [package]
sed -i -E "0,/^version = \".*\"/s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$PROJECT_ROOT/Cargo.toml"

# 2. Atualiza Cargo.lock
cargo check --quiet || cargo update --workspace

echo "==> Versão atualizada no Cargo.toml e Cargo.lock com sucesso!"

# 3. Compilação e Geração dos Pacotes (DEB, RPM, AppImage, Tarball)
echo ""
echo "==> Gerando binários e pacotes locais para a versão $NEW_VERSION..."
"$SCRIPT_DIR/package-all.sh"

# 4. Git Add, Commit, Tag e Push
echo ""
echo "==> Registrando alterações no Git..."
git add -A

TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
BRANCH=$(git branch --show-current || echo "main")
COMMIT_MSG="chore(release): bump version to $NEW_VERSION - $TIMESTAMP"

echo "==> Commitando com mensagem: '$COMMIT_MSG'"
git commit -m "$COMMIT_MSG"

TAG_NAME="v$NEW_VERSION"
echo "==> Criando tag: $TAG_NAME"
# Se tag local já existir, avisa ou substitui com segurança
if git rev-parse "$TAG_NAME" >/dev/null 2>&1; then
  echo "Aviso: Tag $TAG_NAME já existia localmente. Atualizando tag..."
  git tag -f -a "$TAG_NAME" -m "Release $TAG_NAME - $TIMESTAMP"
else
  git tag -a "$TAG_NAME" -m "Release $TAG_NAME - $TIMESTAMP"
fi

echo "==> Enviando commits e tag para origin ($BRANCH)..."
git push origin "$BRANCH"
git push origin "$TAG_NAME"

echo ""
echo "=================================================="
echo "🎉 Release $TAG_NAME concluída com sucesso!"
echo "Artefatos disponíveis em: $PROJECT_ROOT/dist"
ls -lh "$PROJECT_ROOT/dist"
echo "=================================================="
