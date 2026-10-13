<?php

use App\Http\Controllers\BenchController;
use Illuminate\Support\Facades\Route;

Route::get('/plaintext', [BenchController::class, 'plaintext']);
Route::get('/json', [BenchController::class, 'json']);
Route::get('/db', [BenchController::class, 'db']);
Route::get('/page', [BenchController::class, 'page']);
