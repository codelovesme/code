mkdir code-module-demo
cd code-module-demo
curl -fL https://github.com/codelovesme/code/releases/download/v2.23.0/code-v2.23.0-x86_64-linux.tar.gz -o code.tar.gz
tar -xzf code.tar.gz
code_cli="$PWD/code-v2.23.0-x86_64-linux/code"
"$code_cli" --version
