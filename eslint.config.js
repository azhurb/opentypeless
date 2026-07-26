import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'

export default tseslint.config(
  { ignores: ['dist', 'src-tauri'] },
  {
    extends: [js.configs.recommended, ...tseslint.configs.recommended],
    files: ['**/*.{ts,tsx}'],
    plugins: {
      'react-hooks': reactHooks,
      'react-refresh': reactRefresh,
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      'react-refresh/only-export-components': ['warn', { allowConstantExport: true }],
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
      // WKWebView only shows JS dialogs when the host implements WKUIDelegate's
      // runJavaScriptConfirmPanelWithMessage: / AlertPanel: / TextInputPanel: —
      // and wry implements none of them. So on macOS these return falsy (or do
      // nothing) without displaying anything, which silently turned
      // `if (!window.confirm(...)) return` into a permanent early return and made
      // "Clear All History" a no-op. Use src/components/ConfirmDialog.tsx.
      'no-restricted-properties': [
        'error',
        {
          object: 'window',
          property: 'confirm',
          message:
            'Silently returns false on macOS (WKWebView has no JS-dialog delegate in wry). Use ConfirmDialog instead.',
        },
        {
          object: 'window',
          property: 'alert',
          message:
            'Silently does nothing on macOS (WKWebView has no JS-dialog delegate in wry). Use toast() instead.',
        },
        {
          object: 'window',
          property: 'prompt',
          message:
            'Silently returns null on macOS (WKWebView has no JS-dialog delegate in wry). Use an in-app input instead.',
        },
      ],
    },
  },
  {
    files: ['**/*.test.{ts,tsx}', '**/__tests__/**/*.{ts,tsx}'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
    },
  },
)
