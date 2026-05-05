# Welcome
There's no real goal for this readme, i'll just be giving tutorials on how to get started with certain things.

## web servers with Trucky library
```rust
bring trucky
var ui = Trucky()

if request.path == "/" {
    response.body = ui.page("Home", ui.hero("Welcome", "Built with Pitruck", ""))
} else {
    response.status = 404
    response.body = ui.page("Not Found", ui.container(ui.h1("404")))
}
```
