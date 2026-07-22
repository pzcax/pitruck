## 1.5
changelog:
- improved [http client/server](https://blip-docs.pages.dev/#http-server). **DOES NOT SUPPORT HTTPS**, this is coming 1.6
- try/catch 
```py
try {
    var x = 10 / 0
} catch (err) {
    print "Error occurred on line " + to_string(err.line)
    print err.message
}
```
- typeof()
- continue/break
- dict keys accept numbers/bools, null coalescing (`??`), optional chaining ( `?.` and `?[` ) 
- template strings: 
```js
print `this is a ${template} string`
```
- supported string escapes: \n, \t, \r, \e, \", \`, \\, \$, \xHH (Hex), \u{HHHH} (Unicode).
- mutli line strings using 
```py
"""
this
is 
multiline
"""
```
- comments: 
```c
/* multi line comment
   block comment
*/
```
```c
// sinlge line comment
```
```py
# python single line comment
```
- ternary `condition ? true_expr : false_expr`
- fat arrow => syntax 
```js
var multiply = (a, b) => a * b
var block_lambda = (x) => {
    return x * 2
}
```
- JSON built in functions
- official documentation: https://blip-docs.pages.dev/