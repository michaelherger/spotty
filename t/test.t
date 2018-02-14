#!/usr/bin/perl

use strict;

use Data::Dump;
use File::Slurp qw(read_file);
use File::Spec::Functions qw(catdir catfile);
use FindBin qw($Bin);
use JSON;
use Test::More;

use constant TESTTRACKID => '5nAGT4XQVcVPAojSW0PxiL';

my $baseDir = catdir($Bin, '..');
my $cacheDir = catdir($Bin, 'data');
my $credsFile = catfile($cacheDir, 'credentials.json');
my $dataFile = catfile($Bin, 'test-data.json');

mkdir $cacheDir;

plan tests => 17;

my $binary = catdir($baseDir, 'target/debug/spotty');

if (!-e $binary) {
	`cd $baseDir && cargo build`;
	ok(!($? >> 8), "build binary");
}

{
	ok(-f $binary && -e _, "$binary does exist and is executable");
}

$binary = "RUST_BACKTRACE=full $binary -n 'Spotty testing'";

{
	my $checkData = `$binary --check`;
	ok($checkData && $checkData =~ /ok spotty/, 'received response to quick check: ' . $checkData);
}

{
	testCredentials();
}

my $testData = readCredentials($dataFile) || {};

my $username = $ENV{SPOTIFY_USER} || $testData->{username};
my $password = $ENV{SPOTIFY_PASSWORD} || $testData->{password};

if ($username && $password) {
	testCredentials($username, $password);
}

{
	my $cmd = "$binary -c $cacheDir --get-token";
	$cmd .= " -i " . $testData->{client_id} if $testData->{client_id};
	my $tokenData = `$cmd`;
	ok(!($? >> 8), "helper exited token call normally");
	ok($tokenData, "received token data");

	my $token = decode_json($tokenData);
	ok($token && ref $token && $token->{accessToken}, "received accessToken");
}

require Proc::Background;
my $daemon;
{
	$daemon = Proc::Background->new("$binary -c $cacheDir --disable-audio-cache");
	ok($daemon, "daemon started\n");
}

print "\nWe're now going to download some data. Please be patient...\n";
{
	my $testPCM = catfile($cacheDir, 'test.pcm');
	my $streamCmd = sprintf('%s --bitrate=96 -c %s --single-track %s --disable-discovery --disable-audio-cache > %s',
		$binary,
		$cacheDir,
		TESTTRACKID,
		$testPCM
	);

	`$streamCmd`;
	ok(!($? >> 8), "stream helper exited normally");
	ok(-f $testPCM, "audio stream downloaded");
	ok(-s _ > 2_000_000, "downloaded PCM data is of reasonable size: " . -s _);
	unlink $testPCM;
}

{
	ok($daemon->alive, "daemon is still alive and kicking");
	$daemon->die if $daemon->alive;
}


sub testCredentials {
	my ($username, $password) = @_;

	unlink $credsFile;
	if (!$username && !$password) {
		print "\nTesting interactive authentication. Please use Spotify application to authenticate client 'Spotty testing'...\n";
		`$binary -a -c $cacheDir`;
	}
	else {
		print "\nTesting username/password authentication.\n";
		`$binary -a -c $cacheDir -u "$username" -p "$password" --disable-discovery`;
	}

	ok(!($? >> 8), "helper exited normally");
	ok(-f $credsFile, "credentials file does exist");

	my $credentials = readCredentials($credsFile);
	ok($credentials && $credentials->{auth_data}, "credentials file is readable and valid: " . ($credentials && $credentials->{auth_data} ? $credentials->{username} : 'unknown'));
}

sub readCredentials {
	my $file = shift || $credsFile;

	return decode_json(read_file($file));
}

1;