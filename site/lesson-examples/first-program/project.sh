"$code_cli" init hello
cd hello
CODE_MODULE_RELEASE=https://github.com/codelovesme/code/releases/tag/v2.23.0 \
    "$code_cli" install console
cat > main.code <<'CODE'
__HOME_CODE__
CODE
