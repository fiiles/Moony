## Default Permission

What the webview may call by default: `status` and `set_privacy_screen`.

`store`, `retrieve` and `remove` are left out on purpose. They carry the database key, which
only the Rust side handles (through `KeychainExt`, which needs no permission), so the secret
never reaches JavaScript. Grant `moony-keychain:allow-store`, `allow-retrieve` or
`allow-remove` explicitly only for a value the webview is meant to see.

`exclude_from_backup` is left out too: the app's Rust side calls it on its own data folder, and the
webview has no reason to point it at another path.

#### This default permission set includes the following:

- `allow-status`
- `allow-set-privacy-screen`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`moony-keychain:allow-exclude-from-backup`

</td>
<td>

Enables the exclude_from_backup command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-exclude-from-backup`

</td>
<td>

Denies the exclude_from_backup command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:allow-remove`

</td>
<td>

Enables the remove command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-remove`

</td>
<td>

Denies the remove command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:allow-retrieve`

</td>
<td>

Enables the retrieve command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-retrieve`

</td>
<td>

Denies the retrieve command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:allow-set-privacy-screen`

</td>
<td>

Enables the set_privacy_screen command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-set-privacy-screen`

</td>
<td>

Denies the set_privacy_screen command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:allow-status`

</td>
<td>

Enables the status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-status`

</td>
<td>

Denies the status command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:allow-store`

</td>
<td>

Enables the store command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`moony-keychain:deny-store`

</td>
<td>

Denies the store command without any pre-configured scope.

</td>
</tr>
</table>
