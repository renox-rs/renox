<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Items</title></head>
<body>
<h1>Items</h1>
<table>
<tr><th>#</th><th>Name</th><th>Price</th><th>Stock</th></tr>
@foreach ($items as $item)
<tr><td>{{ $item->id }}</td><td>{{ $item->name }}</td><td>{{ $item->price }}</td><td>{{ $item->stock }}</td></tr>
@endforeach
</table>
</body>
</html>
