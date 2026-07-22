const vscode = require('vscode');
const { exec } = require('child_process');

/**
 * @param {vscode.ExtensionContext} context
 */
function activate(context) {
    const SELECTOR = { language: 'pitruck', scheme: 'file' };

    const hoverDocs = {
        'json_encode': '**json_encode(val)**\n\nSerializes a value to a JSON string.',
        'json_decode': '**json_decode(str)**\n\nParses a JSON string into a Pitruck object/dict/list.',
        'len': '**len(str_or_list_or_dict)**\n\nReturns the length of a string, list, or dictionary.',
        'to_string': '**to_string(val)**\n\nConverts a value to its string representation.',
        'to_number': '**to_number(val)**\n\nConverts a string or boolean to a 64-bit float.',
        'typeof': '**typeof(val)**\n\nReturns the type name ("number", "string", "bool", "null", "list", "dict", "function", "class", "instance").',
        'request': '**request** (Global Object)\n\nHTTP Request context provided in `--serve` mode.\n- `request.method`: HTTP Method (e.g. "GET")\n- `request.path`: Target URL path\n- `request.query`: Parsed query parameters dict\n- `request.body`: Raw request body text',
        'response': '**response** (Global Object)\n\nHTTP Response handler provided in `--serve` mode.\n- `response.status`: HTTP status code (Default: 200)\n- `response.body`: Response body string\n- `response.headers`: Output HTTP headers dictionary'
    };

    const hoverProvider = vscode.languages.registerHoverProvider(SELECTOR, {
        provideHover(document, position) {
            const range = document.getWordRangeAtPosition(position);
            if (!range) return;

            const word = document.getText(range);
            if (hoverDocs[word]) {
                return new vscode.Hover(new vscode.MarkdownString(hoverDocs[word]));
            }
        }
    });

    const completionProvider = vscode.languages.registerCompletionItemProvider(
        SELECTOR,
        {
            provideCompletionItems(document, position) {
                const linePrefix = document.lineAt(position).text.substring(0, position.character);
                const items = [];

                if (linePrefix.endsWith('request.')) {
                    const reqProps = ['method', 'path', 'query_str', 'query', 'form', 'body', 'headers'];
                    return reqProps.map(prop => new vscode.CompletionItem(prop, vscode.CompletionItemKind.Property));
                }

                if (linePrefix.endsWith('response.')) {
                    const resProps = ['status', 'body', 'headers'];
                    return resProps.map(prop => new vscode.CompletionItem(prop, vscode.CompletionItemKind.Property));
                }

                // Standard Built-in Functions
                const builtins = [
                    'rand', 'range', 'to_number', 'to_string', 'is_number', 'typeof', 'clone',
                    'len', 'split', 'trim', 'upper', 'lower', 'replace', 'starts_with', 'ends_with',
                    'push', 'pop', 'contains', 'keys', 'values', 'remove', 'join',
                    'json_encode', 'json_decode', 'url_encode', 'url_decode',
                    'time', 'timestamp', 'sys_os', 'sys_exit', 'sys_sleep', 'sys_env'
                ];

                builtins.forEach(fn => {
                    const item = new vscode.CompletionItem(fn, vscode.CompletionItemKind.Function);
                    item.insertText = new vscode.SnippetString(`${fn}($1)`);
                    items.push(item);
                });

                const keywords = ['var', 'bring', 'func', 'return', 'if', 'elif', 'else', 'while', 'for', 'in', 'print', 'class', 'self', 'match', 'request'];
                keywords.forEach(kw => {
                    items.push(new vscode.CompletionItem(kw, vscode.CompletionItemKind.Keyword));
                });

                return items;
            }
        },
        '.' 
    );

    const diagnosticCollection = vscode.languages.createDiagnosticCollection('pitruck');

    function validateDocument(document) {
        if (document.languageId !== 'pitruck') return;

        const filePath = document.fileName;

        exec(`pitruck "${filePath}"`, (error, stdout, stderr) => {
            diagnosticCollection.clear();

            const rawOutput = (stderr || stdout || '').trim();
            if (!rawOutput) return;

            if (error || rawOutput.toLowerCase().includes('error')) {
                const lineMatch = rawOutput.match(/line\s+(\d+)/i);
                let lineNumber = 0;
                if (lineMatch && lineMatch[1]) {
                    lineNumber = Math.max(0, parseInt(lineMatch[1], 10) - 1);
                }

                lineNumber = Math.min(lineNumber, document.lineCount - 1);

                let cleanMessage = rawOutput
                    .replace(/^(\[.*?\]\s*|parse\s+error:\s*|runtime\s+error:\s*)/i, '')
                    .replace(/^line\s+\d+:\s*/i, '')
                    .trim();

                if (cleanMessage.length > 0) {
                    cleanMessage = cleanMessage.charAt(0).toUpperCase() + cleanMessage.slice(1);
                }

                const isUndefinedRequest = /undefined variable ['"]request['"]/i.test(cleanMessage);
                const isUndefinedResponse = /undefined variable ['"]response['"]/i.test(cleanMessage);

                if (isUndefinedRequest || isUndefinedResponse) {
                    return;
                }

                const lineText = document.lineAt(lineNumber).text;
                const range = new vscode.Range(lineNumber, 0, lineNumber, lineText.length);

                const diagnostic = new vscode.Diagnostic(
                    range,
                    cleanMessage || 'Syntax error detected',
                    vscode.DiagnosticSeverity.Error
                );

                diagnosticCollection.set(document.uri, [diagnostic]);
            }
        });
    }

    // Trigger validation on save and file open
    context.subscriptions.push(
        vscode.workspace.onDidSaveTextDocument(validateDocument),
        vscode.workspace.onDidOpenTextDocument(validateDocument)
    );

    context.subscriptions.push(hoverProvider, completionProvider, diagnosticCollection);
}

function deactivate() {}

module.exports = {
    activate,
    deactivate
};