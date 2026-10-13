<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

// The benchmark's four endpoints, as a Laravel app writes them: the query
// builder and Blade, in the `web` middleware group (cookies, the session,
// CSRF), like the Renox app's default stack.
class BenchController extends Controller
{
    private function id(Request $request): int
    {
        return max(1, min(9980, (int) $request->query('id', 1)));
    }

    public function plaintext()
    {
        return response('Hello, World!', 200, ['Content-Type' => 'text/plain; charset=utf-8']);
    }

    public function json()
    {
        return response()->json(['message' => 'Hello, World!']);
    }

    public function db(Request $request)
    {
        $item = DB::table('items')->select('id', 'name', 'price', 'stock')->where('id', $this->id($request))->first();

        return response()->json($item);
    }

    public function page(Request $request)
    {
        $items = DB::table('items')->select('id', 'name', 'price', 'stock')
            ->where('id', '>=', $this->id($request))->orderBy('id')->limit(20)->get();

        return view('page', ['items' => $items]);
    }
}
